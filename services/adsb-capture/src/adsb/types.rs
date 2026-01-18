//! ADS-B data types

/// Downlink format identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DownlinkFormat {
    ShortAirSurveillance = 0,
    AltitudeReply = 4,
    IdentityReply = 5,
    AllCallReply = 11,
    LongAirSurveillance = 16,
    ExtendedSquitter = 17,
    ExtendedSquitterNonTransponder = 18,
    MilitaryExtendedSquitter = 19,
    CommBAltitude = 20,
    CommBIdentity = 21,
    Unknown = 255,
}

impl From<u8> for DownlinkFormat {
    fn from(df: u8) -> Self {
        match df {
            0 => Self::ShortAirSurveillance,
            4 => Self::AltitudeReply,
            5 => Self::IdentityReply,
            11 => Self::AllCallReply,
            16 => Self::LongAirSurveillance,
            17 => Self::ExtendedSquitter,
            18 => Self::ExtendedSquitterNonTransponder,
            19 => Self::MilitaryExtendedSquitter,
            20 => Self::CommBAltitude,
            21 => Self::CommBIdentity,
            _ => Self::Unknown,
        }
    }
}

/// Parsed aircraft data from ADS-B message
#[derive(Debug, Clone, Default)]
pub struct AircraftData {
    /// ICAO 24-bit address
    pub icao_address: u32,

    /// Flight callsign (8 characters max)
    pub callsign: Option<String>,

    /// Latitude in degrees (-90 to 90)
    pub latitude: Option<f64>,

    /// Longitude in degrees (-180 to 180)
    pub longitude: Option<f64>,

    /// Barometric altitude in feet
    pub altitude_ft: Option<i32>,

    /// Ground speed in knots
    pub ground_speed_kts: Option<f32>,

    /// True heading in degrees (0-360)
    pub heading_deg: Option<f32>,

    /// Vertical rate in feet per minute
    pub vertical_rate_fpm: Option<i32>,

    /// Squawk code (4-digit octal)
    pub squawk: Option<u16>,

    /// Downlink format
    pub df: u8,

    /// Type code (for DF17/18)
    pub tc: u8,

    /// Whether altitude is from GNSS (true) or barometric (false)
    pub altitude_gnss: bool,
}
