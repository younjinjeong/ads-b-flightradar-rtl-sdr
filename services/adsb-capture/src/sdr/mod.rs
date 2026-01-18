//! Native RTL-SDR capture and Mode S/ADS-B demodulation
//!
//! This module provides dump1090-style decoding:
//! 1. Capture raw IQ samples from RTL-SDR at 2 MSPS
//! 2. Convert to magnitude (sqrt(I² + Q²))
//! 3. Detect Mode S preambles
//! 4. Extract and decode frames
//! 5. Verify CRC-24

pub mod capture;
mod demod;
mod detect;

pub use capture::{query_device_serial, query_device_info, SdrCapture, SdrConfig};
pub use demod::MagnitudeTable;
pub use detect::{DetectorStats, Frame};
