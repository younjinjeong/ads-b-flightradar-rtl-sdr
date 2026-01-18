//! Aircraft state tracking and aggregation
//!
//! Aggregates partial ADS-B data from multiple messages into complete aircraft state.
//! This is essential for weak signal conditions where individual messages may be incomplete.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, info};

use std::collections::VecDeque;

/// Maximum age for aircraft state before removal
const AIRCRAFT_TIMEOUT_SECS: u64 = 60;

/// Position update threshold for logging
const POSITION_LOG_INTERVAL_SECS: u64 = 5;

/// Maximum recent messages to keep for deduplication
const MAX_RECENT_MESSAGES: usize = 10;

/// Recent message for deduplication and voting
#[derive(Debug, Clone)]
struct RecentMessage {
    /// Message hash (simplified - just compare raw bytes)
    hash: u64,
    /// Timestamp
    time: Instant,
    /// Position data if available
    lat: Option<f64>,
    lon: Option<f64>,
    alt: Option<i32>,
}

/// Aggregated aircraft state
#[derive(Debug, Clone)]
pub struct AircraftState {
    /// ICAO 24-bit address
    pub icao: u32,
    /// Flight callsign
    pub callsign: Option<String>,
    /// Last known latitude
    pub latitude: Option<f64>,
    /// Last known longitude
    pub longitude: Option<f64>,
    /// Barometric altitude in feet
    pub altitude_ft: Option<i32>,
    /// Ground speed in knots
    pub ground_speed_kts: Option<f32>,
    /// True heading in degrees
    pub heading_deg: Option<f32>,
    /// Vertical rate in feet per minute
    pub vertical_rate_fpm: Option<i32>,
    /// Squawk code
    pub squawk: Option<u16>,
    /// Last update time
    pub last_seen: Instant,
    /// Last position update time (for rate limiting logs)
    pub last_position_log: Instant,
    /// Message count
    pub messages: u64,
    /// Position message count
    pub position_messages: u64,
    /// Whether we have a valid position
    pub has_position: bool,
    /// Recent messages for deduplication
    recent_messages: VecDeque<RecentMessage>,
    /// Confidence score (higher = more reliable)
    pub confidence: u32,
}

impl AircraftState {
    pub fn new(icao: u32) -> Self {
        let now = Instant::now();
        Self {
            icao,
            callsign: None,
            latitude: None,
            longitude: None,
            altitude_ft: None,
            ground_speed_kts: None,
            heading_deg: None,
            vertical_rate_fpm: None,
            squawk: None,
            last_seen: now,
            last_position_log: now - Duration::from_secs(POSITION_LOG_INTERVAL_SECS),
            messages: 0,
            position_messages: 0,
            has_position: false,
            recent_messages: VecDeque::with_capacity(MAX_RECENT_MESSAGES),
            confidence: 0,
        }
    }

    /// Update state with new aircraft data
    pub fn update(&mut self, data: &crate::adsb::AircraftData) {
        self.last_seen = Instant::now();
        self.messages += 1;

        // Create message hash for deduplication
        let msg_hash = Self::compute_message_hash(data);

        // Check for duplicate message (same data within 1 second)
        let is_duplicate = self.recent_messages.iter().any(|m| {
            m.hash == msg_hash && m.time.elapsed() < Duration::from_secs(1)
        });

        if is_duplicate {
            // Duplicate message confirms previous data - increase confidence
            self.confidence = self.confidence.saturating_add(1);
            return;
        }

        // Add to recent messages
        self.recent_messages.push_back(RecentMessage {
            hash: msg_hash,
            time: Instant::now(),
            lat: data.latitude,
            lon: data.longitude,
            alt: data.altitude_ft,
        });
        while self.recent_messages.len() > MAX_RECENT_MESSAGES {
            self.recent_messages.pop_front();
        }

        // Update callsign if provided
        if let Some(ref cs) = data.callsign {
            if !cs.trim().is_empty() && cs != "#######" {
                self.callsign = Some(cs.clone());
            }
        }

        // Update position if provided
        if data.latitude.is_some() && data.longitude.is_some() {
            let new_lat = data.latitude.unwrap();
            let new_lon = data.longitude.unwrap();

            // Validate position (basic sanity check)
            if new_lat.abs() <= 90.0 && new_lon.abs() <= 180.0 {
                // Reasonableness check: verify position is physically possible
                if let (Some(old_lat), Some(old_lon)) = (self.latitude, self.longitude) {
                    let time_delta = self.last_seen.elapsed().as_secs_f64();
                    if time_delta > 0.0 && time_delta < 60.0 {
                        // Calculate distance in nautical miles (approximate)
                        let distance_nm = Self::haversine_distance_nm(old_lat, old_lon, new_lat, new_lon);

                        // Max speed: 900 knots = 15 nm/second
                        let max_distance = 15.0 * time_delta;

                        if distance_nm > max_distance {
                            // Position jump too large - likely noise/error
                            // Don't update position, but still count the message
                            return;
                        }
                    }
                }

                self.latitude = Some(new_lat);
                self.longitude = Some(new_lon);
                self.position_messages += 1;
                self.has_position = true;
            }
        }

        // Update altitude if provided
        if let Some(alt) = data.altitude_ft {
            if alt > -2000 && alt < 60000 {
                self.altitude_ft = Some(alt);
            }
        }

        // Update velocity if provided
        if let Some(speed) = data.ground_speed_kts {
            if speed >= 0.0 && speed < 1000.0 {
                self.ground_speed_kts = Some(speed);
            }
        }

        if let Some(hdg) = data.heading_deg {
            if hdg >= 0.0 && hdg < 360.0 {
                self.heading_deg = Some(hdg);
            }
        }

        if let Some(vr) = data.vertical_rate_fpm {
            if vr.abs() < 10000 {
                self.vertical_rate_fpm = Some(vr);
            }
        }

        // Update squawk if provided
        if let Some(sq) = data.squawk {
            self.squawk = Some(sq);
        }
    }

    /// Check if enough time has passed to log position again
    pub fn should_log_position(&self) -> bool {
        self.last_position_log.elapsed() >= Duration::from_secs(POSITION_LOG_INTERVAL_SECS)
    }

    /// Mark position as logged
    pub fn mark_position_logged(&mut self) {
        self.last_position_log = Instant::now();
    }

    /// Check if aircraft state is stale
    pub fn is_stale(&self) -> bool {
        self.last_seen.elapsed() > Duration::from_secs(AIRCRAFT_TIMEOUT_SECS)
    }

    /// Get age in seconds
    pub fn age_secs(&self) -> u64 {
        self.last_seen.elapsed().as_secs()
    }

    /// Compute a simple hash for message deduplication
    fn compute_message_hash(data: &crate::adsb::AircraftData) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        data.icao_address.hash(&mut hasher);

        // Hash position data if available
        if let Some(lat) = data.latitude {
            ((lat * 10000.0) as i64).hash(&mut hasher);
        }
        if let Some(lon) = data.longitude {
            ((lon * 10000.0) as i64).hash(&mut hasher);
        }
        if let Some(alt) = data.altitude_ft {
            alt.hash(&mut hasher);
        }

        // Hash callsign if available
        if let Some(ref cs) = data.callsign {
            cs.hash(&mut hasher);
        }

        // Hash velocity data
        if let Some(spd) = data.ground_speed_kts {
            ((spd * 10.0) as i32).hash(&mut hasher);
        }
        if let Some(hdg) = data.heading_deg {
            ((hdg * 10.0) as i32).hash(&mut hasher);
        }

        hasher.finish()
    }

    /// Calculate haversine distance between two lat/lon points in nautical miles
    fn haversine_distance_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
        const EARTH_RADIUS_NM: f64 = 3440.065; // Earth radius in nautical miles

        let lat1_rad = lat1.to_radians();
        let lat2_rad = lat2.to_radians();
        let delta_lat = (lat2 - lat1).to_radians();
        let delta_lon = (lon2 - lon1).to_radians();

        let a = (delta_lat / 2.0).sin().powi(2)
            + lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().asin();

        EARTH_RADIUS_NM * c
    }
}

/// Aircraft tracker - manages state for all tracked aircraft
pub struct AircraftTracker {
    aircraft: HashMap<u32, AircraftState>,
    max_aircraft: usize,
    last_cleanup: Instant,
}

impl AircraftTracker {
    pub fn new(max_aircraft: usize) -> Self {
        Self {
            aircraft: HashMap::with_capacity(max_aircraft),
            max_aircraft,
            last_cleanup: Instant::now(),
        }
    }

    /// Update aircraft state with new data, returns updated state if significant
    pub fn update(&mut self, data: &crate::adsb::AircraftData) -> Option<&AircraftState> {
        let icao = data.icao_address;

        // Get or create aircraft state
        if !self.aircraft.contains_key(&icao) {
            // Check capacity
            if self.aircraft.len() >= self.max_aircraft {
                self.cleanup_stale();
            }
            self.aircraft.insert(icao, AircraftState::new(icao));
            debug!("New aircraft tracked: {:06X}", icao);
        }

        let state = self.aircraft.get_mut(&icao)?;
        let had_position = state.has_position;

        state.update(data);

        // Log if we got a new position or it's time for an update
        if state.has_position && ((!had_position) || state.should_log_position()) {
            state.mark_position_logged();
            info!(
                "Aircraft {:06X} {} at ({:.4}, {:.4}) alt={} spd={:.0} hdg={:.0} | msgs={}",
                icao,
                state.callsign.as_deref().unwrap_or("-"),
                state.latitude.unwrap_or(0.0),
                state.longitude.unwrap_or(0.0),
                state.altitude_ft.unwrap_or(0),
                state.ground_speed_kts.unwrap_or(0.0),
                state.heading_deg.unwrap_or(0.0),
                state.messages
            );
        }

        // Periodic cleanup
        if self.last_cleanup.elapsed() > Duration::from_secs(30) {
            self.cleanup_stale();
            self.last_cleanup = Instant::now();
        }

        self.aircraft.get(&icao)
    }

    /// Get aircraft state by ICAO
    pub fn get(&self, icao: u32) -> Option<&AircraftState> {
        self.aircraft.get(&icao)
    }

    /// Get all active aircraft
    pub fn get_all(&self) -> impl Iterator<Item = &AircraftState> {
        self.aircraft.values().filter(|a| !a.is_stale())
    }

    /// Get aircraft with valid positions
    pub fn get_with_positions(&self) -> impl Iterator<Item = &AircraftState> {
        self.aircraft.values().filter(|a| a.has_position && !a.is_stale())
    }

    /// Get count of tracked aircraft
    pub fn count(&self) -> usize {
        self.aircraft.len()
    }

    /// Get count of aircraft with positions
    pub fn count_with_positions(&self) -> usize {
        self.aircraft.values().filter(|a| a.has_position && !a.is_stale()).count()
    }

    /// Remove stale aircraft
    fn cleanup_stale(&mut self) {
        let before = self.aircraft.len();
        self.aircraft.retain(|_, state| !state.is_stale());
        let removed = before - self.aircraft.len();
        if removed > 0 {
            debug!("Cleaned up {} stale aircraft, {} remaining", removed, self.aircraft.len());
        }
    }

    /// Get summary statistics
    pub fn stats_summary(&self) -> TrackerStats {
        let total = self.aircraft.len();
        let with_position = self.count_with_positions();
        let with_callsign = self.aircraft.values().filter(|a| a.callsign.is_some() && !a.is_stale()).count();
        let total_messages: u64 = self.aircraft.values().map(|a| a.messages).sum();

        TrackerStats {
            total_aircraft: total,
            with_position,
            with_callsign,
            total_messages,
        }
    }
}

/// Tracker statistics
#[derive(Debug, Clone)]
pub struct TrackerStats {
    pub total_aircraft: usize,
    pub with_position: usize,
    pub with_callsign: usize,
    pub total_messages: u64,
}

impl std::fmt::Display for TrackerStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Aircraft: {} total, {} with position, {} with callsign, {} msgs",
            self.total_aircraft, self.with_position, self.with_callsign, self.total_messages
        )
    }
}
