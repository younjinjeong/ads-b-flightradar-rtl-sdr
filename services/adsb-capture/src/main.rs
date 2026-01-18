//! ADS-B Capture - Native RTL-SDR with dump1090-style decoder
//!
//! Captures raw IQ samples from RTL-SDR, demodulates and decodes Mode S/ADS-B,
//! and streams decoded data to grpc-gateway.

mod adsb;
mod aircraft_tracker;
mod config;
mod decoder;
mod device;
mod grpc;
mod sdr;

use aircraft_tracker::AircraftTracker;

use anyhow::Result;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;

use config::Config;
use grpc::adsb::{AircraftEvent, DeviceStatus, SignalMetrics};
use grpc::StreamingGatewayClient;
use sdr::{query_device_info, SdrCapture, SdrConfig};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .with_target(false)
        .init();

    info!("===========================================");
    info!("   ADS-B Capture - Native RTL-SDR");
    info!("   dump1090-style Rust decoder");
    info!("===========================================");

    // Load configuration
    let mut config = Config::from_env();

    // Determine rtl_sdr path for device query
    let rtl_sdr_path = config.rtl_adsb_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.join("rtl_sdr.exe"))
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("lib")
                .join("rtl_sdr.exe")
        });

    // Query device info unless DEVICE_ID was explicitly set (doesn't start with RTL-SDR-)
    let device_id_from_env = std::env::var("DEVICE_ID").is_ok();
    if !device_id_from_env {
        info!("Querying RTL-SDR device info...");
        let (manufacturer, product, serial) = query_device_info(
            rtl_sdr_path.to_string_lossy().as_ref(),
            config.device_index
        );

        if let Some(sn) = &serial {
            config.device_id = format!("RTL-SDR-{}", sn);
            info!("  Device ID: {}", sn);
        }
        if let Some(mfr) = &manufacturer {
            info!("  Manufacturer: {}", mfr);
        }
        if let Some(prd) = &product {
            info!("  Product: {}", prd);
        }
        if serial.is_none() {
            info!("  Could not query device info, using default ID");
        }
    } else {
        info!("Using user-specified DEVICE_ID: {}", config.device_id);
    }

    info!("Configuration:");
    info!("  Gateway URL: {}", config.gateway_url);
    info!("  Device index: {}", config.device_index);
    info!("  Device ID: {}", config.device_id);
    info!("  Gain: {} dB", config.gain_db);
    info!("  PPM error: {}", config.ppm_error);

    // Create channels for data flow to gRPC gateway
    let (aircraft_tx, aircraft_rx) = mpsc::channel::<AircraftEvent>(1000);
    let (signal_tx, signal_rx) = mpsc::channel::<SignalMetrics>(100);
    let (status_tx, status_rx) = mpsc::channel::<DeviceStatus>(10);

    // Start gRPC streaming to gateway
    let gateway_url = config.gateway_url.clone();
    let aircraft_handle = tokio::spawn(async move {
        let client = StreamingGatewayClient::new(&gateway_url);
        if let Err(e) = client.stream_aircraft(aircraft_rx).await {
            error!("Aircraft stream failed: {}", e);
        }
    });

    let gateway_url = config.gateway_url.clone();
    let signal_handle = tokio::spawn(async move {
        let client = StreamingGatewayClient::new(&gateway_url);
        if let Err(e) = client.stream_signal(signal_rx).await {
            error!("Signal stream failed: {}", e);
        }
    });

    let gateway_url = config.gateway_url.clone();
    let status_handle = tokio::spawn(async move {
        let client = StreamingGatewayClient::new(&gateway_url);
        if let Err(e) = client.stream_status(status_rx).await {
            error!("Status stream failed: {}", e);
        }
    });

    // Configure SDR capture via rtl_sdr.exe process
    // rtl_sdr_path was already determined above for device query
    info!("rtl_sdr path: {:?}", rtl_sdr_path);

    let sdr_config = SdrConfig {
        device_index: config.device_index,
        center_freq: 1_090_000_000,
        sample_rate: 2_000_000,
        gain: (config.gain_db * 10.0) as i32, // Convert to tenths of dB
        ppm_error: config.ppm_error,
        rtl_sdr_path: rtl_sdr_path.to_string_lossy().to_string(),
    };

    // Start native SDR capture
    let sdr = SdrCapture::new(sdr_config);
    let frame_rx = match sdr.start() {
        Ok(rx) => rx,
        Err(e) => {
            error!("Failed to start SDR capture: {}", e);
            error!("Make sure RTL-SDR device is connected and drivers are installed.");
            return Err(e);
        }
    };

    // Send initial device status
    let initial_status = DeviceStatus {
        device_id: config.device_id.clone(),
        connected: true,
        sample_rate: 2_000_000,
        center_freq: 1_090_000_000,
        gain_db: config.gain_db,
        timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
    };
    let _ = status_tx.send(initial_status).await;

    info!("===========================================");
    info!("  Starting capture...");
    info!("  Press Ctrl+C to stop.");
    info!("===========================================");

    // CPR context for position decoding
    let mut cpr_context = adsb::CprContext::new(256);

    // Aircraft tracker for state aggregation
    let mut aircraft_tracker = AircraftTracker::new(256);

    // Track statistics
    let mut frames_processed = 0u64;
    let mut last_heartbeat = Instant::now();
    let mut last_signal_report = Instant::now();
    let mut last_tracker_report = Instant::now();

    // Main processing loop - receive decoded frames from SDR
    loop {
        // Non-blocking receive with timeout for heartbeats
        match frame_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(frame) => {
                frames_processed += 1;

                // Parse the raw frame into aircraft data
                match adsb::parse_message(&frame.data, &mut cpr_context) {
                    Ok(aircraft) => {
                        // Update aircraft tracker (aggregates all data per ICAO)
                        if let Some(state) = aircraft_tracker.update(&aircraft) {
                            // Build aircraft event from aggregated state
                            let event = AircraftEvent {
                                device_id: config.device_id.clone(),
                                timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
                                icao: format!("{:06X}", state.icao),
                                callsign: state.callsign.clone().unwrap_or_default(),
                                altitude_ft: state.altitude_ft.unwrap_or(0),
                                latitude: state.latitude.unwrap_or(0.0),
                                longitude: state.longitude.unwrap_or(0.0),
                                speed_kts: state.ground_speed_kts.unwrap_or(0.0),
                                heading_deg: state.heading_deg.unwrap_or(0.0),
                                vertical_rate_fpm: state.vertical_rate_fpm.unwrap_or(0),
                                squawk: state.squawk.map(|s| format!("{:04}", s)).unwrap_or_default(),
                                downlink_format: aircraft.df as u32,
                                type_code: aircraft.tc as u32,
                            };

                            // Send to gateway (only if we have useful data)
                            if state.has_position || state.callsign.is_some() || state.altitude_ft.is_some() {
                                if let Err(e) = aircraft_tx.send(event).await {
                                    warn!("Failed to send aircraft event: {}", e);
                                }
                            }
                        }
                    }
                    Err(adsb::ParseError::CrcError) => {
                        // CRC already verified in detector, shouldn't happen
                    }
                    Err(_) => {
                        // Other parse errors - frame decoded but not interpretable
                    }
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                // No frame received, continue with periodic tasks
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                warn!("SDR frame channel disconnected");
                break;
            }
        }

        // Periodic heartbeat (every 5 seconds to keep status "active" in DB)
        // The DB considers device active if last_heartbeat < 30 seconds ago
        if last_heartbeat.elapsed() >= Duration::from_secs(5) {
            let status = DeviceStatus {
                device_id: config.device_id.clone(),
                connected: sdr.is_running(),
                sample_rate: 2_000_000,
                center_freq: 1_090_000_000,
                gain_db: config.gain_db,
                timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            };
            let _ = status_tx.send(status).await;
            last_heartbeat = Instant::now();
        }

        // Periodic signal metrics (every 500ms)
        if last_signal_report.elapsed() >= Duration::from_millis(500) {
            let stats = sdr.stats();
            let elapsed = last_signal_report.elapsed().as_secs_f32();

            // Get detector statistics
            let noise_floor = stats.noise_floor.load(std::sync::atomic::Ordering::Relaxed);
            let peak_signal = stats.peak_signal.load(std::sync::atomic::Ordering::Relaxed);
            let preambles = stats.preambles_detected.load(std::sync::atomic::Ordering::Relaxed);
            let frames = stats.frames_detected.load(std::sync::atomic::Ordering::Relaxed);
            let crc_errors = stats.crc_errors.load(std::sync::atomic::Ordering::Relaxed);
            let corrected = stats.corrected_frames.load(std::sync::atomic::Ordering::Relaxed);
            let samples_processed = stats.samples_captured.load(std::sync::atomic::Ordering::Relaxed);

            // Convert magnitude to dBFS (8-bit unsigned IQ, max magnitude ~362 for full scale)
            // dBFS = 20 * log10(magnitude / max_magnitude)
            // For RTL-SDR 8-bit IQ: max magnitude = sqrt(127^2 + 127^2) ≈ 180
            let max_possible: f32 = 180.0;
            let signal_dbfs = if peak_signal > 0 {
                20.0 * (peak_signal as f32 / max_possible).log10()
            } else {
                -60.0
            };
            let noise_dbfs = if noise_floor > 0 {
                20.0 * (noise_floor as f32 / max_possible).log10()
            } else {
                -60.0
            };
            let snr_db = signal_dbfs - noise_dbfs;

            let metrics = SignalMetrics {
                device_id: config.device_id.clone(),
                timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
                signal_dbfs,
                noise_dbfs,
                snr_db,
                msg_rate: frames_processed as f32 / elapsed.max(1.0),
                preambles_detected: preambles,
                frames_decoded: frames,
                crc_errors,
                corrected_frames: corrected,
                samples_processed,
                noise_floor,
                peak_signal,
            };
            let _ = signal_tx.send(metrics).await;
            last_signal_report = Instant::now();
        }

        // Periodic tracker statistics (every 10 seconds)
        if last_tracker_report.elapsed() >= Duration::from_secs(10) {
            let stats = aircraft_tracker.stats_summary();
            info!(
                "[Tracker] {}",
                stats
            );
            last_tracker_report = Instant::now();
        }

        // Check if SDR is still running
        if !sdr.is_running() {
            warn!("SDR capture stopped unexpectedly");
            break;
        }
    }

    // Cleanup
    sdr.stop();

    // Send disconnected status
    let final_status = DeviceStatus {
        device_id: config.device_id.clone(),
        connected: false,
        sample_rate: 2_000_000,
        center_freq: 1_090_000_000,
        gain_db: config.gain_db,
        timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
    };
    let _ = status_tx.send(final_status).await;

    // Cancel streaming tasks
    aircraft_handle.abort();
    signal_handle.abort();
    status_handle.abort();

    info!("Shutdown complete. Frames processed: {}", frames_processed);
    Ok(())
}
