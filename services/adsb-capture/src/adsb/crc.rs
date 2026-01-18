//! CRC-24 checksum validation for Mode S messages

/// CRC-24 polynomial used in Mode S (0x1FFF409)
const CRC24_POLY: u32 = 0x1FFF409;

/// Compute CRC-24 checksum over message bytes
pub fn compute_crc24(msg: &[u8], bits: usize) -> u32 {
    let bytes = bits / 8;
    let mut crc: u32 = 0;

    for i in 0..bytes {
        crc ^= (msg[i] as u32) << 16;

        for _ in 0..8 {
            if crc & 0x800000 != 0 {
                crc = (crc << 1) ^ CRC24_POLY;
            } else {
                crc <<= 1;
            }
        }
    }

    crc & 0xFFFFFF
}

/// Check CRC validity of an ADS-B message
/// Returns Ok(()) if valid, Err(()) if invalid
///
/// STRICT MODE: Only accepts DF=11, 17, 18 (ADS-B) where CRC can be fully verified.
/// This prevents false positives from noise being interpreted as Mode S frames.
pub fn check_crc(msg: &[u8]) -> Result<(), ()> {
    let len = msg.len();
    if len != 7 && len != 14 {
        return Err(());
    }

    let df = (msg[0] >> 3) & 0x1F;

    // Only accept long frames (14 bytes)
    if len != 14 {
        return Err(());
    }

    // For DF=11, 17, 18: CRC is computed over whole message and should be 0
    // These are the only formats we can reliably verify with weak signals
    if df == 11 || df == 17 || df == 18 {
        let full_crc = compute_crc24(msg, 112);
        if full_crc == 0 {
            Ok(())
        } else {
            Err(())
        }
    } else {
        // Reject all other formats to avoid false positives
        // With weak signals, we can't reliably verify DF0,4,5,16,20,21,24
        Err(())
    }
}

/// Extract ICAO address from message (bytes 1-3)
pub fn get_icao(msg: &[u8]) -> u32 {
    ((msg[1] as u32) << 16) | ((msg[2] as u32) << 8) | (msg[3] as u32)
}

/// Extract downlink format from message
pub fn get_df(msg: &[u8]) -> u8 {
    (msg[0] >> 3) & 0x1F
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc24() {
        // Test vector from ADS-B specification
        let msg = hex::decode("8D4840D6202CC371C32CE0576098").unwrap();
        let crc = compute_crc24(&msg, 112);
        assert_eq!(crc, 0); // Valid message should have CRC of 0
    }

    #[test]
    fn test_get_icao() {
        let msg = hex::decode("8D4840D6202CC371C32CE0576098").unwrap();
        assert_eq!(get_icao(&msg), 0x4840D6);
    }

    #[test]
    fn test_get_df() {
        let msg = hex::decode("8D4840D6202CC371C32CE0576098").unwrap();
        assert_eq!(get_df(&msg), 17); // DF17 = Extended Squitter
    }
}
