//! gRPC client for streaming to gateway

use anyhow::Result;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Channel;
use tracing::{info, warn};

use super::adsb::{
    adsb_gateway_client::AdsbGatewayClient, AircraftEvent, DeviceStatus, SignalMetrics,
};

/// Streaming gateway client with automatic reconnection
pub struct StreamingGatewayClient {
    gateway_url: String,
}

impl StreamingGatewayClient {
    pub fn new(gateway_url: &str) -> Self {
        Self {
            gateway_url: gateway_url.to_string(),
        }
    }

    /// Connect to gateway with retry
    async fn connect_with_retry(&self, stream_name: &str) -> Channel {
        info!("[{}] Connecting to gateway: {}", stream_name, self.gateway_url);
        loop {
            match Channel::from_shared(self.gateway_url.clone()) {
                Ok(endpoint) => match endpoint.connect().await {
                    Ok(ch) => {
                        info!("[{}] Connected to gateway successfully", stream_name);
                        return ch;
                    }
                    Err(e) => {
                        warn!("[{}] Failed to connect to gateway: {}. Retrying in 2s...", stream_name, e);
                    }
                },
                Err(e) => {
                    warn!("[{}] Invalid gateway URL: {}. Retrying in 2s...", stream_name, e);
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    }

    /// Stream aircraft events to gateway (takes ownership of receiver)
    pub async fn stream_aircraft(
        &self,
        rx: mpsc::Receiver<AircraftEvent>,
    ) -> Result<()> {
        // Connect first, then stream
        let channel = self.connect_with_retry("Aircraft").await;
        let mut client = AdsbGatewayClient::new(channel);
        info!("[Aircraft] Starting stream to gateway...");
        let stream = ReceiverStream::new(rx);

        match client.stream_aircraft(stream).await {
            Ok(response) => {
                info!("[Aircraft] Stream ended: {:?}", response.into_inner());
                Ok(())
            }
            Err(e) => {
                warn!("[Aircraft] Stream error: {}", e);
                Err(e.into())
            }
        }
    }

    /// Stream signal metrics to gateway
    pub async fn stream_signal(
        &self,
        rx: mpsc::Receiver<SignalMetrics>,
    ) -> Result<()> {
        let channel = self.connect_with_retry("Signal").await;
        let mut client = AdsbGatewayClient::new(channel);
        info!("[Signal] Starting stream to gateway...");
        let stream = ReceiverStream::new(rx);

        match client.stream_signal(stream).await {
            Ok(response) => {
                info!("[Signal] Stream ended: {:?}", response.into_inner());
                Ok(())
            }
            Err(e) => {
                warn!("[Signal] Stream error: {}", e);
                Err(e.into())
            }
        }
    }

    /// Stream device status to gateway
    pub async fn stream_status(
        &self,
        rx: mpsc::Receiver<DeviceStatus>,
    ) -> Result<()> {
        let channel = self.connect_with_retry("Status").await;
        let mut client = AdsbGatewayClient::new(channel);
        info!("[Status] Starting stream to gateway...");
        let stream = ReceiverStream::new(rx);

        match client.stream_device_status(stream).await {
            Ok(response) => {
                info!("[Status] Stream ended: {:?}", response.into_inner());
                Ok(())
            }
            Err(e) => {
                warn!("[Status] Stream error: {}", e);
                Err(e.into())
            }
        }
    }
}
