//! Configuration loaded from environment variables

use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// Gateway URL for gRPC streaming
    pub gateway_url: String,

    /// RTL-SDR device index
    pub device_index: u32,

    /// Device ID string for identification
    pub device_id: String,

    /// Tuner gain in dB (use 0 for auto)
    pub gain_db: f32,

    /// PPM frequency correction
    pub ppm_error: i32,

    /// Path to rtl_adsb executable
    pub rtl_adsb_path: PathBuf,

    /// Signal metrics reporting interval in milliseconds
    pub signal_report_interval_ms: u64,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            gateway_url: std::env::var("GATEWAY_URL")
                .unwrap_or_else(|_| "http://localhost:30051".to_string()),

            device_index: std::env::var("DEVICE_INDEX")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),

            device_id: std::env::var("DEVICE_ID")
                .unwrap_or_else(|_| format!("RTL-SDR-{:08X}", 1)),

            gain_db: std::env::var("DEVICE_GAIN")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(49.6),

            ppm_error: std::env::var("PPM_ERROR")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),

            rtl_adsb_path: std::env::var("RTL_ADSB_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("rtl_adsb.exe")),

            signal_report_interval_ms: std::env::var("SIGNAL_REPORT_INTERVAL_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(500),  // 0.5 seconds for real-time signal updates
        }
    }
}
