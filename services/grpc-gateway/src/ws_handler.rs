//! WebSocket handler for real-time updates to browser clients

use crate::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

/// Handle WebSocket upgrade request
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Handle individual WebSocket connection
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to broadcast channel
    let mut broadcast_rx = state.broadcast_tx.subscribe();

    info!("New WebSocket client connected");

    // Send initial aircraft list
    match state.db_writer.get_current_aircraft().await {
        Ok(aircraft) => {
            let initial_msg = serde_json::json!({
                "type": "initial",
                "aircraft": aircraft,
            });
            if let Ok(json) = serde_json::to_string(&initial_msg) {
                if sender.send(Message::Text(json)).await.is_err() {
                    return;
                }
            }
        }
        Err(e) => {
            error!("Failed to get initial aircraft: {}", e);
        }
    }

    // Send current SDR device status
    match state.db_writer.get_sdr_status().await {
        Ok(status) => {
            let status_msg = serde_json::json!({
                "type": "device_status",
                "device_id": status.get("device_id").and_then(|v| v.as_str()).unwrap_or("unknown"),
                "connected": status.get("connected").and_then(|v| v.as_bool()).unwrap_or(false),
                "sample_rate": status.get("sample_rate").and_then(|v| v.as_i64()).unwrap_or(0),
                "center_freq": status.get("center_freq").and_then(|v| v.as_i64()).unwrap_or(0),
                "gain_db": status.get("gain_db").and_then(|v| v.as_f64()).unwrap_or(0.0),
            });
            if let Ok(json) = serde_json::to_string(&status_msg) {
                if sender.send(Message::Text(json)).await.is_err() {
                    return;
                }
            }
        }
        Err(e) => {
            debug!("No SDR status available: {}", e);
        }
    }

    // Spawn task to forward broadcasts to this client
    let mut send_task = tokio::spawn(async move {
        loop {
            match broadcast_rx.recv().await {
                Ok(msg) => {
                    if sender.send(Message::Text(msg)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    debug!("WebSocket client lagged by {} messages", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    });

    // Handle incoming messages from client
    let mut recv_task = tokio::spawn(async move {
        while let Some(result) = receiver.next().await {
            match result {
                Ok(Message::Text(text)) => {
                    // Handle client messages (subscribe, ping, etc.)
                    debug!("Received from client: {}", text);
                    if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) {
                        match msg.get("type").and_then(|t| t.as_str()) {
                            Some("subscribe") => {
                                // Client wants to subscribe (we already send everything)
                                debug!("Client subscribed");
                            }
                            Some("ping") => {
                                debug!("Client ping");
                            }
                            _ => {}
                        }
                    }
                }
                Ok(Message::Ping(_)) => {
                    // Handled automatically by axum
                }
                Ok(Message::Close(_)) => {
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    // Wait for either task to complete
    tokio::select! {
        _ = &mut send_task => {
            recv_task.abort();
        }
        _ = &mut recv_task => {
            send_task.abort();
        }
    }

    info!("WebSocket client disconnected");
}
