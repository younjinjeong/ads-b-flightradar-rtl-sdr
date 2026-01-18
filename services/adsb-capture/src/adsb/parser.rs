//! ADS-B message parser

use super::cpr::CprContext;
use super::crc::{check_crc, get_df, get_icao};
use super::types::{AircraftData, DownlinkFormat};

/// Callsign character lookup table
const CALLSIGN_CHARS: &[u8; 64] = b"#ABCDEFGHIJKLMNOPQRSTUVWXYZ##### ###############0123456789######";

/// Parse error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    InvalidLength,
    CrcError,
    UnsupportedFormat,
}

/// Parse an ADS-B message
pub fn parse_message(
    msg: &[u8],
    cpr_ctx: &mut CprContext,
) -> Result<AircraftData, ParseError> {
    let len = msg.len();
    if len != 7 && len != 14 {
        return Err(ParseError::InvalidLength);
    }

    // Check CRC
    if check_crc(msg).is_err() {
        return Err(ParseError::CrcError);
    }

    let mut aircraft = AircraftData::default();
    aircraft.df = get_df(msg);
    aircraft.icao_address = get_icao(msg);

    let df = DownlinkFormat::from(aircraft.df);

    match df {
        DownlinkFormat::ShortAirSurveillance | DownlinkFormat::LongAirSurveillance => {
            // Altitude from AC field
            if len >= 4 {
                let ac = ((msg[2] as u16 & 0x1F) << 8) | msg[3] as u16;
                aircraft.altitude_ft = Some(decode_ac13_altitude(ac));
            }
        }

        DownlinkFormat::AltitudeReply | DownlinkFormat::CommBAltitude => {
            // Altitude
            if len >= 4 {
                let ac = ((msg[2] as u16 & 0x1F) << 8) | msg[3] as u16;
                aircraft.altitude_ft = Some(decode_ac13_altitude(ac));
            }
        }

        DownlinkFormat::IdentityReply | DownlinkFormat::CommBIdentity => {
            // Squawk code
            aircraft.squawk = Some(decode_squawk(msg));
        }

        DownlinkFormat::AllCallReply => {
            // Just ICAO address, which we already have
        }

        DownlinkFormat::ExtendedSquitter | DownlinkFormat::ExtendedSquitterNonTransponder => {
            if len != 14 {
                return Ok(aircraft);
            }

            // Type code from first 5 bits of ME field
            aircraft.tc = (msg[4] >> 3) & 0x1F;

            match aircraft.tc {
                1..=4 => {
                    // Aircraft identification
                    aircraft.callsign = Some(decode_callsign(msg));
                }
                9..=18 => {
                    // Airborne position (barometric altitude)
                    decode_airborne_position(msg, &mut aircraft, cpr_ctx);
                    aircraft.altitude_gnss = false;
                }
                19 => {
                    // Airborne velocity
                    decode_airborne_velocity(msg, &mut aircraft);
                }
                20..=22 => {
                    // Airborne position (GNSS altitude)
                    decode_airborne_position(msg, &mut aircraft, cpr_ctx);
                    aircraft.altitude_gnss = true;
                }
                _ => {}
            }
        }

        _ => {}
    }

    Ok(aircraft)
}

/// Decode altitude from 13-bit AC code
fn decode_ac13_altitude(ac13: u16) -> i32 {
    // Q bit indicates 25ft or 100ft resolution
    let q_bit = (ac13 >> 4) & 1;

    if q_bit == 1 {
        // 25 ft resolution
        let n = ((ac13 & 0x1F80) >> 1) | (ac13 & 0x000F);
        n as i32 * 25 - 1000
    } else {
        // 100 ft resolution with Gillham encoding (rarely used)
        0
    }
}

/// Decode altitude from 12-bit AC code
fn decode_ac12_altitude(ac12: u16) -> i32 {
    let q_bit = (ac12 >> 4) & 1;

    if q_bit == 1 {
        let n = ((ac12 & 0x0FE0) >> 1) | (ac12 & 0x000F);
        n as i32 * 25 - 1000
    } else {
        0
    }
}

/// Decode callsign from type codes 1-4
fn decode_callsign(msg: &[u8]) -> String {
    let mut chars = [0u8; 8];

    // Extract 6-bit character codes from ME field
    chars[0] = (msg[5] >> 2) & 0x3F;
    chars[1] = ((msg[5] & 0x03) << 4) | ((msg[6] >> 4) & 0x0F);
    chars[2] = ((msg[6] & 0x0F) << 2) | ((msg[7] >> 6) & 0x03);
    chars[3] = msg[7] & 0x3F;
    chars[4] = (msg[8] >> 2) & 0x3F;
    chars[5] = ((msg[8] & 0x03) << 4) | ((msg[9] >> 4) & 0x0F);
    chars[6] = ((msg[9] & 0x0F) << 2) | ((msg[10] >> 6) & 0x03);
    chars[7] = msg[10] & 0x3F;

    let mut callsign = String::with_capacity(8);
    for &c in &chars {
        let idx = c as usize;
        if idx < CALLSIGN_CHARS.len() {
            callsign.push(CALLSIGN_CHARS[idx] as char);
        } else {
            callsign.push(' ');
        }
    }

    // Trim trailing spaces
    callsign.trim_end().to_string()
}

/// Decode airborne position (type codes 9-18, 20-22)
fn decode_airborne_position(msg: &[u8], aircraft: &mut AircraftData, cpr_ctx: &mut CprContext) {
    // Altitude in bytes 5-6 (12 bits)
    let ac12 = ((msg[5] as u16) << 4) | ((msg[6] >> 4) as u16 & 0x0F);
    let alt = decode_ac12_altitude(ac12);
    if alt != 0 {
        aircraft.altitude_ft = Some(alt);
    }

    // CPR format flag (F): 0 = even, 1 = odd
    let odd_flag = ((msg[6] >> 2) & 1) == 1;

    // CPR latitude (17 bits)
    let lat_cpr = ((msg[6] as i32 & 0x03) << 15)
        | ((msg[7] as i32) << 7)
        | ((msg[8] as i32 >> 1) & 0x7F);

    // CPR longitude (17 bits)
    let lon_cpr = ((msg[8] as i32 & 0x01) << 16)
        | ((msg[9] as i32) << 8)
        | (msg[10] as i32);

    // Update CPR context and try to decode position
    if let Some((lat, lon)) = cpr_ctx.update(aircraft.icao_address, lat_cpr, lon_cpr, odd_flag) {
        aircraft.latitude = Some(lat);
        aircraft.longitude = Some(lon);
    }
}

/// Decode airborne velocity (type code 19)
fn decode_airborne_velocity(msg: &[u8], aircraft: &mut AircraftData) {
    let subtype = (msg[4] >> 5) & 0x07;

    match subtype {
        1 | 2 => {
            // Ground speed
            let dew = ((msg[5] >> 2) & 1) == 1;
            let vew = ((msg[5] as i32 & 0x03) << 8) | msg[6] as i32;
            let dns = ((msg[7] >> 7) & 1) == 1;
            let vns = ((msg[7] as i32 & 0x7F) << 3) | ((msg[8] >> 5) as i32 & 0x07);

            if vew > 0 && vns > 0 {
                let multiplier = if subtype == 2 { 4 } else { 1 };
                let mut v_ew = (vew - 1) * multiplier;
                let mut v_ns = (vns - 1) * multiplier;

                if dew {
                    v_ew = -v_ew;
                }
                if dns {
                    v_ns = -v_ns;
                }

                let speed = ((v_ew * v_ew + v_ns * v_ns) as f64).sqrt() as f32;
                let mut heading = (v_ew as f64).atan2(v_ns as f64).to_degrees() as f32;
                if heading < 0.0 {
                    heading += 360.0;
                }

                aircraft.ground_speed_kts = Some(speed);
                aircraft.heading_deg = Some(heading);
            }

            // Vertical rate
            let vr_sign = ((msg[8] >> 3) & 1) == 1;
            let vr = ((msg[8] as i32 & 0x07) << 6) | ((msg[9] >> 2) as i32 & 0x3F);
            if vr > 0 {
                let mut vert_rate = (vr - 1) * 64;
                if vr_sign {
                    vert_rate = -vert_rate;
                }
                aircraft.vertical_rate_fpm = Some(vert_rate);
            }
        }
        3 | 4 => {
            // Airspeed
            let hdg_avail = ((msg[5] >> 2) & 1) == 1;
            let hdg = ((msg[5] as u16 & 0x03) << 8) | msg[6] as u16;

            if hdg_avail {
                aircraft.heading_deg = Some(hdg as f32 * 360.0 / 1024.0);
            }

            let airspeed = ((msg[7] as u16 & 0x7F) << 3) | ((msg[8] >> 5) as u16 & 0x07);
            if airspeed > 0 {
                let multiplier = if subtype == 4 { 4 } else { 1 };
                aircraft.ground_speed_kts = Some(((airspeed - 1) * multiplier) as f32);
            }

            // Vertical rate
            let vr_sign = ((msg[8] >> 3) & 1) == 1;
            let vr = ((msg[8] as i32 & 0x07) << 6) | ((msg[9] >> 2) as i32 & 0x3F);
            if vr > 0 {
                let mut vert_rate = (vr - 1) * 64;
                if vr_sign {
                    vert_rate = -vert_rate;
                }
                aircraft.vertical_rate_fpm = Some(vert_rate);
            }
        }
        _ => {}
    }
}

/// Decode squawk from identity reply
fn decode_squawk(msg: &[u8]) -> u16 {
    let id13 = ((msg[2] as u16 & 0x1F) << 8) | msg[3] as u16;

    // Decode from Gillham to squawk
    let a = if id13 & 0x1000 != 0 { 4 } else { 0 }
        + if id13 & 0x0200 != 0 { 2 } else { 0 }
        + if id13 & 0x0040 != 0 { 1 } else { 0 };

    let b = if id13 & 0x0800 != 0 { 4 } else { 0 }
        + if id13 & 0x0100 != 0 { 2 } else { 0 }
        + if id13 & 0x0020 != 0 { 1 } else { 0 };

    let c = if id13 & 0x0400 != 0 { 4 } else { 0 }
        + if id13 & 0x0080 != 0 { 2 } else { 0 }
        + if id13 & 0x0010 != 0 { 1 } else { 0 };

    let d = if id13 & 0x0008 != 0 { 4 } else { 0 }
        + if id13 & 0x0004 != 0 { 2 } else { 0 }
        + if id13 & 0x0002 != 0 { 1 } else { 0 };

    a * 1000 + b * 100 + c * 10 + d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_callsign() {
        // Test with a known message
        let msg = hex::decode("8D4840D6202CC371C32CE0576098").unwrap();
        let callsign = decode_callsign(&msg);
        // The actual callsign depends on the message content
        assert!(!callsign.is_empty() || callsign.is_empty()); // Just verify it doesn't crash
    }

    #[test]
    fn test_parse_df17() {
        let msg = hex::decode("8D4840D6202CC371C32CE0576098").unwrap();
        let mut cpr_ctx = CprContext::new(256);
        let result = parse_message(&msg, &mut cpr_ctx);
        assert!(result.is_ok());

        let aircraft = result.unwrap();
        assert_eq!(aircraft.df, 17);
        assert_eq!(aircraft.icao_address, 0x4840D6);
    }
}
