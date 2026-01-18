//! CPR (Compact Position Reporting) position decoding

use std::collections::HashMap;
use std::time::Instant;

/// CPR state for a single aircraft
#[derive(Debug, Clone)]
pub struct CprState {
    /// Even CPR coordinates and timestamp
    pub even_cpr: Option<(i32, i32, Instant)>,
    /// Odd CPR coordinates and timestamp
    pub odd_cpr: Option<(i32, i32, Instant)>,
    /// Last decoded position
    pub last_position: Option<(f64, f64)>,
}

impl Default for CprState {
    fn default() -> Self {
        Self {
            even_cpr: None,
            odd_cpr: None,
            last_position: None,
        }
    }
}

/// Context for CPR decoding across multiple aircraft
pub struct CprContext {
    states: HashMap<u32, CprState>,
    max_aircraft: usize,
}

impl CprContext {
    pub fn new(max_aircraft: usize) -> Self {
        Self {
            states: HashMap::with_capacity(max_aircraft),
            max_aircraft,
        }
    }

    /// Get or create CPR state for an aircraft
    pub fn get_or_create(&mut self, icao: u32) -> &mut CprState {
        // Evict oldest if at capacity
        if self.states.len() >= self.max_aircraft && !self.states.contains_key(&icao) {
            // Simple eviction: remove first entry
            if let Some(&first_key) = self.states.keys().next() {
                self.states.remove(&first_key);
            }
        }

        self.states.entry(icao).or_default()
    }

    /// Update CPR data and attempt position decode
    pub fn update(
        &mut self,
        icao: u32,
        lat_cpr: i32,
        lon_cpr: i32,
        odd_flag: bool,
    ) -> Option<(f64, f64)> {
        let state = self.get_or_create(icao);
        let now = Instant::now();

        if odd_flag {
            state.odd_cpr = Some((lat_cpr, lon_cpr, now));
        } else {
            state.even_cpr = Some((lat_cpr, lon_cpr, now));
        }

        // Try global decoding
        decode_global(state, odd_flag)
    }
}

/// NL (Number of Longitude zones) lookup function
/// Returns the number of longitude zones at a given latitude
fn cpr_nl(lat: f64) -> i32 {
    let lat = lat.abs();

    if lat < 10.47047130 { return 59; }
    if lat < 14.82817437 { return 58; }
    if lat < 18.18626357 { return 57; }
    if lat < 21.02939493 { return 56; }
    if lat < 23.54504487 { return 55; }
    if lat < 25.82924707 { return 54; }
    if lat < 27.93898710 { return 53; }
    if lat < 29.91135686 { return 52; }
    if lat < 31.77209708 { return 51; }
    if lat < 33.53993436 { return 50; }
    if lat < 35.22899598 { return 49; }
    if lat < 36.85025108 { return 48; }
    if lat < 38.41241892 { return 47; }
    if lat < 39.92256684 { return 46; }
    if lat < 41.38651832 { return 45; }
    if lat < 42.80914012 { return 44; }
    if lat < 44.19454951 { return 43; }
    if lat < 45.54626723 { return 42; }
    if lat < 46.86733252 { return 41; }
    if lat < 48.16039128 { return 40; }
    if lat < 49.42776439 { return 39; }
    if lat < 50.67150166 { return 38; }
    if lat < 51.89342469 { return 37; }
    if lat < 53.09516153 { return 36; }
    if lat < 54.27817472 { return 35; }
    if lat < 55.44378444 { return 34; }
    if lat < 56.59318756 { return 33; }
    if lat < 57.72747354 { return 32; }
    if lat < 58.84763776 { return 31; }
    if lat < 59.95459277 { return 30; }
    if lat < 61.04917774 { return 29; }
    if lat < 62.13216659 { return 28; }
    if lat < 63.20427479 { return 27; }
    if lat < 64.26616523 { return 26; }
    if lat < 65.31845310 { return 25; }
    if lat < 66.36171008 { return 24; }
    if lat < 67.39646774 { return 23; }
    if lat < 68.42322022 { return 22; }
    if lat < 69.44242631 { return 21; }
    if lat < 70.45451075 { return 20; }
    if lat < 71.45986473 { return 19; }
    if lat < 72.45884545 { return 18; }
    if lat < 73.45177442 { return 17; }
    if lat < 74.43893416 { return 16; }
    if lat < 75.42056257 { return 15; }
    if lat < 76.39684391 { return 14; }
    if lat < 77.36789461 { return 13; }
    if lat < 78.33374083 { return 12; }
    if lat < 79.29428225 { return 11; }
    if lat < 80.24923213 { return 10; }
    if lat < 81.19801349 { return 9; }
    if lat < 82.13956981 { return 8; }
    if lat < 83.07199445 { return 7; }
    if lat < 83.99173563 { return 6; }
    if lat < 84.89166191 { return 5; }
    if lat < 85.75541621 { return 4; }
    if lat < 86.53536998 { return 3; }
    if lat < 87.00000000 { return 2; }
    1
}

/// Decode CPR position using global decoding
/// Requires both even and odd messages within 10 seconds
fn decode_global(state: &mut CprState, odd_flag: bool) -> Option<(f64, f64)> {
    let (even_lat, even_lon, even_time) = state.even_cpr?;
    let (odd_lat, odd_lon, odd_time) = state.odd_cpr?;

    // Check time validity (10 seconds max between even/odd)
    let time_diff = if odd_flag {
        even_time.elapsed()
    } else {
        odd_time.elapsed()
    };

    if time_diff.as_secs() > 10 {
        return None;
    }

    // CPR decoding algorithm
    let lat_cpr_even = even_lat as f64 / 131072.0;
    let lon_cpr_even = even_lon as f64 / 131072.0;
    let lat_cpr_odd = odd_lat as f64 / 131072.0;
    let lon_cpr_odd = odd_lon as f64 / 131072.0;

    // Latitude zone sizes
    let dlat_even = 360.0 / 60.0;
    let dlat_odd = 360.0 / 59.0;

    // Compute latitude index
    let j = (59.0 * lat_cpr_even - 60.0 * lat_cpr_odd + 0.5).floor() as i32;

    let mut lat_even = dlat_even * ((j % 60) as f64 + lat_cpr_even);
    let mut lat_odd = dlat_odd * ((j % 59) as f64 + lat_cpr_odd);

    if lat_even >= 270.0 {
        lat_even -= 360.0;
    }
    if lat_odd >= 270.0 {
        lat_odd -= 360.0;
    }

    // Check latitude zone consistency
    let nl_even = cpr_nl(lat_even);
    let nl_odd = cpr_nl(lat_odd);
    if nl_even != nl_odd {
        return None; // Different zones, can't decode
    }

    let (lat, lon) = if odd_flag {
        // Use odd message (most recent)
        let nl = nl_odd;
        let mut ni = nl - 1;
        if ni < 1 {
            ni = 1;
        }
        let dlon = 360.0 / ni as f64;

        let m = (lon_cpr_even * (nl - 1) as f64 - lon_cpr_odd * nl as f64 + 0.5).floor() as i32;
        let lon = dlon * ((m % ni) as f64 + lon_cpr_odd);

        (lat_odd, lon)
    } else {
        // Use even message (most recent)
        let nl = nl_even;
        let mut ni = nl;
        if ni < 1 {
            ni = 1;
        }
        let dlon = 360.0 / ni as f64;

        let m = (lon_cpr_even * (nl - 1) as f64 - lon_cpr_odd * nl as f64 + 0.5).floor() as i32;
        let lon = dlon * ((m % ni) as f64 + lon_cpr_even);

        (lat_even, lon)
    };

    // Normalize longitude
    let lon = if lon > 180.0 { lon - 360.0 } else { lon };

    // Validate result
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }

    // Save for future local decoding
    state.last_position = Some((lat, lon));

    Some((lat, lon))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpr_nl() {
        assert_eq!(cpr_nl(0.0), 59);
        assert_eq!(cpr_nl(45.0), 42);
        assert_eq!(cpr_nl(87.0), 2);
    }
}
