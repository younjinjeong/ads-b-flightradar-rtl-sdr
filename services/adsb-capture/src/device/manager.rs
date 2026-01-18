//! Device manager - coordinates decoder and message processing

use std::time::Instant;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::adsb::{parse_message, AircraftData, CprContext, ParseError};
use crate::config::Config;
use crate::decoder::DecoderRunner;
use crate::grpc::adsb::{AircraftEvent, DeviceStatus, SignalMetrics};

use super::state::DeviceState;

/// Device manager coordinates decoder and message processing
pub struct DeviceManager {
    config: Config,
    device_state: DeviceState,
    cpr_context: CprContext,
    aircraft_tx: mpsc::Sender<AircraftEvent>,
    signal_tx: mpsc::Sender<SignalMetrics>,
    status_tx: mpsc::Sender<DeviceStatus>,
}

impl DeviceManager {
    pub fn new(
        config: Config,
        aircraft_tx: mpsc::Sender<AircraftEvent>,
        signal_tx: mpsc::Sender<SignalMetrics>,
        status_tx: mpsc::Sender<DeviceStatus>,
    ) -> Self {
        let device_state = DeviceState::new(
            config.device_id.clone(),
            config.device_index,
            config.gain_db,
        );

        Self {
            config,
            device_state,
            cpr_context: CprContext::new(256),
            aircraft_tx,
            signal_tx,
            status_tx,
        }
    }

    /// Run the device manager
    pub async fn run(mut self) -> Result<()> {
        info!("Starting device manager for {}", self.config.device_id);

        // Create channel for raw messages from decoder
        let (raw_tx, mut raw_rx) = mpsc::channel::<Vec<u8>>(1000);

        // Create decoder runner
        let decoder = DecoderRunner::new(
            &self.config.rtl_adsb_path,
            self.config.device_index,
            self.config.gain_db,
            self.config.ppm_error,
        );

        // Start decoder in background task
        let decoder_handle = tokio::spawn(async move {
            if let Err(e) = decoder.run(raw_tx).await {
                error!("Decoder error: {}", e);
            }
        });

        // Send initial device status
        self.device_state.connected = true;
        self.send_device_status().await;

        // Track for signal metrics and periodic updates
        let mut last_signal_report = Instant::now();
        let mut last_status_log = Instant::now();
        let mut last_device_status = Instant::now();
        let mut messages_since_report = 0u64;
        let mut aircraft_count_since_log = 0u64;

        // Create a periodic tick for heartbeats (fires every 5 seconds)
        let mut heartbeat_interval = tokio::time::interval(tokio::time::Duration::from_secs(5));

        // Process messages
        loop {
            tokio::select! {
                Some(raw_msg) = raw_rx.recv() => {
                    match parse_message(&raw_msg, &mut self.cpr_context) {
                        Ok(aircraft) => {
                            self.device_state.stats.record_decoded();
                            messages_since_report += 1;
                            aircraft_count_since_log += 1;

                            // Log aircraft detection with details
                            if let Some(ref callsign) = aircraft.callsign {
                                if aircraft.latitude.is_some() && aircraft.longitude.is_some() {
                                    info!(
                                        "Aircraft {:06X} ({}) at ({:.4}, {:.4}) alt={} ft",
                                        aircraft.icao_address,
                                        callsign,
                                        aircraft.latitude.unwrap(),
                                        aircraft.longitude.unwrap(),
                                        aircraft.altitude_ft.unwrap_or(0)
                                    );
                                } else {
                                    debug!(
                                        "Aircraft {:06X} ({}) - no position yet",
                                        aircraft.icao_address, callsign
                                    );
                                }
                            }

                            // Convert to protobuf and send
                            if let Err(e) = self.send_aircraft_event(&aircraft).await {
                                warn!("Failed to send aircraft event: {}", e);
                            } else {
                                self.device_state.stats.record_sent();
                            }
                        }
                        Err(ParseError::CrcError) => {
                            self.device_state.stats.record_crc_error();
                        }
                        Err(_) => {
                            // Other parse errors, ignore
                        }
                    }

                    // Send periodic signal metrics when messages are received
                    if last_signal_report.elapsed().as_millis() >= self.config.signal_report_interval_ms as u128 {
                        let elapsed_sec = last_signal_report.elapsed().as_secs_f32();
                        let msg_rate = if elapsed_sec > 0.0 {
                            messages_since_report as f32 / elapsed_sec
                        } else {
                            0.0
                        };

                        self.send_signal_metrics(msg_rate).await;
                        last_signal_report = Instant::now();
                        messages_since_report = 0;
                    }

                    // Log periodic status summary every 10 seconds
                    if last_status_log.elapsed().as_secs() >= 10 {
                        info!(
                            "[Stats] Aircraft events: {} | Total decoded: {} | Sent: {} | CRC errors: {}",
                            aircraft_count_since_log,
                            self.device_state.stats.get_decoded(),
                            self.device_state.stats.get_sent(),
                            self.device_state.stats.get_crc_errors()
                        );
                        last_status_log = Instant::now();
                        aircraft_count_since_log = 0;
                    }
                }
                // Periodic heartbeat timer - sends updates even when no messages arrive
                _ = heartbeat_interval.tick() => {
                    // Send device status heartbeat every 15 seconds
                    if last_device_status.elapsed().as_secs() >= 15 {
                        debug!("Sending device status heartbeat");
                        self.send_device_status().await;
                        last_device_status = Instant::now();
                    }

                    // Send signal metrics (with 0 rate if no messages) periodically
                    if last_signal_report.elapsed().as_millis() >= self.config.signal_report_interval_ms as u128 {
                        let elapsed_sec = last_signal_report.elapsed().as_secs_f32();
                        let msg_rate = if elapsed_sec > 0.0 {
                            messages_since_report as f32 / elapsed_sec
                        } else {
                            0.0
                        };

                        self.send_signal_metrics(msg_rate).await;
                        last_signal_report = Instant::now();
                        messages_since_report = 0;
                    }
                }
                else => {
                    info!("Raw message channel closed");
                    break;
                }
            }
        }

        // Send disconnected status
        self.device_state.connected = false;
        self.send_device_status().await;

        // Wait for decoder to finish
        let _ = decoder_handle.await;

        info!(
            "Device manager stopped. Decoded: {}, Sent: {}, CRC errors: {}",
            self.device_state.stats.get_decoded(),
            self.device_state.stats.get_sent(),
            self.device_state.stats.get_crc_errors(),
        );

        Ok(())
    }

    /// Convert AircraftData to protobuf and send
    async fn send_aircraft_event(&self, aircraft: &AircraftData) -> Result<()> {
        let event = AircraftEvent {
            device_id: self.device_state.device_id.clone(),
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            icao: format!("{:06X}", aircraft.icao_address),
            callsign: aircraft.callsign.clone().unwrap_or_default(),
            altitude_ft: aircraft.altitude_ft.unwrap_or(0),
            latitude: aircraft.latitude.unwrap_or(0.0),
            longitude: aircraft.longitude.unwrap_or(0.0),
            speed_kts: aircraft.ground_speed_kts.unwrap_or(0.0),
            heading_deg: aircraft.heading_deg.unwrap_or(0.0),
            vertical_rate_fpm: aircraft.vertical_rate_fpm.unwrap_or(0),
            squawk: aircraft.squawk.map(|s| format!("{:04}", s)).unwrap_or_default(),
            downlink_format: aircraft.df as u32,
            type_code: aircraft.tc as u32,
        };

        self.aircraft_tx.send(event).await?;
        Ok(())
    }

    /// Send signal metrics
    async fn send_signal_metrics(&self, msg_rate: f32) {
        let metrics = SignalMetrics {
            device_id: self.device_state.device_id.clone(),
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            signal_dbfs: -30.0,  // Placeholder - would need IQ data for real estimate
            noise_dbfs: -45.0,   // Placeholder
            snr_db: 15.0,        // Placeholder
            msg_rate,
            // New fields - not available in this legacy code path
            preambles_detected: 0,
            frames_decoded: 0,
            crc_errors: 0,
            corrected_frames: 0,
            samples_processed: 0,
            noise_floor: 0,
            peak_signal: 0,
        };

        if let Err(e) = self.signal_tx.send(metrics).await {
            debug!("Failed to send signal metrics: {}", e);
        }
    }

    /// Send device status
    async fn send_device_status(&self) {
        let status = DeviceStatus {
            device_id: self.device_state.device_id.clone(),
            connected: self.device_state.connected,
            sample_rate: self.device_state.sample_rate,
            center_freq: self.device_state.center_freq,
            gain_db: self.device_state.gain_db,
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
        };

        if let Err(e) = self.status_tx.send(status).await {
            warn!("Failed to send device status: {}", e);
        }
    }
}
