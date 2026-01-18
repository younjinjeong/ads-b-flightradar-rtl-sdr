//! gRPC server implementation - receives streams from host

use crate::adsb::{
    adsb_gateway_server::AdsbGateway, AircraftEvent, DeviceStatus, SignalMetrics, StreamAck,
};
use crate::db_writer::DbWriter;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status, Streaming};
use tracing::{debug, error, info, warn};

/// gRPC Gateway service implementation
pub struct GatewayService {
    db_writer: Arc<DbWriter>,
    broadcast_tx: Arc<broadcast::Sender<String>>,
}

impl GatewayService {
    pub fn new(
        db_writer: Arc<DbWriter>,
        broadcast_tx: Arc<broadcast::Sender<String>>,
    ) -> Self {
        Self {
            db_writer,
            broadcast_tx,
        }
    }

    /// Broadcast a JSON message to all WebSocket clients
    fn broadcast_json(&self, json: &str) {
        if self.broadcast_tx.receiver_count() > 0 {
            let _ = self.broadcast_tx.send(json.to_string());
        }
    }
}

#[tonic::async_trait]
impl AdsbGateway for GatewayService {
    /// Receive aircraft events from host, store in DB and broadcast
    async fn stream_aircraft(
        &self,
        request: Request<Streaming<AircraftEvent>>,
    ) -> Result<Response<StreamAck>, Status> {
        let peer = request
            .remote_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        info!("New aircraft stream from {}", peer);

        let mut stream = request.into_inner();
        let mut count = 0u64;
        let mut errors = 0u64;

        while let Some(result) = stream.next().await {
            match result {
                Ok(event) => {
                    count += 1;

                    debug!(
                        "Aircraft: icao={}, pos=({}, {}), alt={}",
                        event.icao, event.latitude, event.longitude, event.altitude_ft
                    );

                    // Store in database
                    if let Err(e) = self.db_writer.insert_position(&event).await {
                        warn!("Failed to insert position: {}", e);
                        errors += 1;
                    }

                    // Broadcast to WebSocket clients
                    let ws_msg = serde_json::json!({
                        "type": "position_update",
                        "icao": event.icao,
                        "device_id": event.device_id,
                        "lat": event.latitude,
                        "lon": event.longitude,
                        "altitude": event.altitude_ft,
                        "speed": event.speed_kts,
                        "heading": event.heading_deg,
                        "vrate": event.vertical_rate_fpm,
                        "callsign": event.callsign,
                        "squawk": event.squawk,
                        "timestamp_ms": event.timestamp_ms,
                    });
                    if let Ok(json) = serde_json::to_string(&ws_msg) {
                        self.broadcast_json(&json);
                    }

                    // Log progress periodically
                    if count % 100 == 0 {
                        info!("Aircraft stream: received={}, errors={}", count, errors);
                    }
                }
                Err(e) => {
                    error!("Stream error: {}", e);
                    errors += 1;
                }
            }
        }

        info!(
            "Aircraft stream from {} ended: received={}, errors={}",
            peer, count, errors
        );

        Ok(Response::new(StreamAck {
            success: true,
            message: format!("Received {} aircraft events", count),
            messages_received: count,
        }))
    }

    /// Receive signal metrics from host, broadcast only (ephemeral)
    async fn stream_signal(
        &self,
        request: Request<Streaming<SignalMetrics>>,
    ) -> Result<Response<StreamAck>, Status> {
        let peer = request
            .remote_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        info!("New signal stream from {}", peer);

        let mut stream = request.into_inner();
        let mut count = 0u64;

        while let Some(result) = stream.next().await {
            match result {
                Ok(metrics) => {
                    count += 1;

                    debug!(
                        "Signal: device={}, signal={:.1}dB, noise={:.1}dB, snr={:.1}dB",
                        metrics.device_id, metrics.signal_dbfs, metrics.noise_dbfs, metrics.snr_db
                    );

                    // Broadcast to WebSocket clients (ephemeral - not stored)
                    let ws_msg = serde_json::json!({
                        "type": "signal",
                        "device_id": metrics.device_id,
                        "signal_dbfs": metrics.signal_dbfs,
                        "noise_dbfs": metrics.noise_dbfs,
                        "snr_db": metrics.snr_db,
                        "msg_rate": metrics.msg_rate,
                        "timestamp_ms": metrics.timestamp_ms,
                        // Decoder statistics
                        "preambles_detected": metrics.preambles_detected,
                        "frames_decoded": metrics.frames_decoded,
                        "crc_errors": metrics.crc_errors,
                        "corrected_frames": metrics.corrected_frames,
                        "samples_processed": metrics.samples_processed,
                        "noise_floor": metrics.noise_floor,
                        "peak_signal": metrics.peak_signal,
                    });
                    if let Ok(json) = serde_json::to_string(&ws_msg) {
                        self.broadcast_json(&json);
                    }
                }
                Err(e) => {
                    warn!("Signal stream error: {}", e);
                }
            }
        }

        info!("Signal stream from {} ended: received={}", peer, count);

        Ok(Response::new(StreamAck {
            success: true,
            message: format!("Received {} signal metrics", count),
            messages_received: count,
        }))
    }

    /// Receive device status from host, store and broadcast
    async fn stream_device_status(
        &self,
        request: Request<Streaming<DeviceStatus>>,
    ) -> Result<Response<StreamAck>, Status> {
        let peer = request
            .remote_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        info!("New device status stream from {}", peer);

        let mut stream = request.into_inner();
        let mut count = 0u64;

        while let Some(result) = stream.next().await {
            match result {
                Ok(status) => {
                    count += 1;

                    info!(
                        "Device status: id={}, connected={}, freq={}, gain={:.1}dB",
                        status.device_id, status.connected, status.center_freq, status.gain_db
                    );

                    // Store in database
                    if let Err(e) = self.db_writer.update_sdr_status(&status).await {
                        warn!("Failed to update SDR status: {}", e);
                    }

                    // Broadcast to WebSocket clients
                    let ws_msg = serde_json::json!({
                        "type": "device_status",
                        "device_id": status.device_id,
                        "connected": status.connected,
                        "sample_rate": status.sample_rate,
                        "center_freq": status.center_freq,
                        "gain_db": status.gain_db,
                        "timestamp_ms": status.timestamp_ms,
                    });
                    if let Ok(json) = serde_json::to_string(&ws_msg) {
                        self.broadcast_json(&json);
                    }
                }
                Err(e) => {
                    warn!("Device status stream error: {}", e);
                }
            }
        }

        info!("Device status stream from {} ended: received={}", peer, count);

        Ok(Response::new(StreamAck {
            success: true,
            message: format!("Received {} device status updates", count),
            messages_received: count,
        }))
    }
}
