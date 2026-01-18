//! ADS-B message parsing module

mod crc;
mod cpr;
pub mod parser;
mod types;

pub use cpr::CprContext;
pub use parser::{parse_message, ParseError};
pub use types::AircraftData;

/// Verify CRC of a Mode S message (exposed for SDR decoder)
pub fn verify_crc(data: &[u8]) -> bool {
    crc::check_crc(data).is_ok()
}
