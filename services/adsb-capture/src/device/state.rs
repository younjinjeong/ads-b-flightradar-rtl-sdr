//! Per-device state tracking

use std::sync::atomic::{AtomicU64, Ordering};

/// Statistics for a single device
#[derive(Debug, Default)]
pub struct DeviceStats {
    pub messages_decoded: AtomicU64,
    pub messages_sent: AtomicU64,
    pub crc_errors: AtomicU64,
}

impl DeviceStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_decoded(&self) {
        self.messages_decoded.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_sent(&self) {
        self.messages_sent.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_crc_error(&self) {
        self.crc_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get_decoded(&self) -> u64 {
        self.messages_decoded.load(Ordering::Relaxed)
    }

    pub fn get_sent(&self) -> u64 {
        self.messages_sent.load(Ordering::Relaxed)
    }

    pub fn get_crc_errors(&self) -> u64 {
        self.crc_errors.load(Ordering::Relaxed)
    }
}

/// State for a single RTL-SDR device
pub struct DeviceState {
    pub device_id: String,
    pub device_index: u32,
    pub stats: DeviceStats,
    pub connected: bool,
    pub sample_rate: u32,
    pub center_freq: u64,
    pub gain_db: f32,
}

impl DeviceState {
    pub fn new(device_id: String, device_index: u32, gain_db: f32) -> Self {
        Self {
            device_id,
            device_index,
            stats: DeviceStats::new(),
            connected: false,
            sample_rate: 2_000_000,  // rtl_adsb uses 2 MSPS
            center_freq: 1_090_000_000,  // ADS-B frequency
            gain_db,
        }
    }
}
