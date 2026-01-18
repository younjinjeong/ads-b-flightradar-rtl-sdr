//! Database writer for TimescaleDB

use crate::adsb::{AircraftEvent, DeviceStatus};
use anyhow::Result;
use deadpool_postgres::{Config, Pool, Runtime};
use serde_json::Value as JsonValue;
use tokio_postgres::NoTls;
use tracing::{debug, warn};

/// Database writer with connection pooling
pub struct DbWriter {
    pool: Option<Pool>,
}

impl DbWriter {
    /// Create a new database writer
    pub async fn new(db_url: &str) -> Result<Self> {
        // Parse connection string
        let mut config = Config::new();

        for part in db_url.split_whitespace() {
            let kv: Vec<&str> = part.splitn(2, '=').collect();
            if kv.len() == 2 {
                match kv[0] {
                    "host" => config.host = Some(kv[1].to_string()),
                    "port" => config.port = Some(kv[1].parse().unwrap_or(5432)),
                    "dbname" => config.dbname = Some(kv[1].to_string()),
                    "user" => config.user = Some(kv[1].to_string()),
                    "password" => config.password = Some(kv[1].to_string()),
                    _ => {}
                }
            }
        }

        let pool = config.create_pool(Some(Runtime::Tokio1), NoTls)?;

        // Test connection
        let client = pool.get().await?;
        client.execute("SELECT 1", &[]).await?;

        Ok(Self { pool: Some(pool) })
    }

    /// Create a dummy writer (no database)
    pub fn new_dummy() -> Self {
        Self { pool: None }
    }

    /// Check if database is available
    fn has_db(&self) -> bool {
        self.pool.is_some()
    }

    /// Insert aircraft position
    pub async fn insert_position(&self, event: &AircraftEvent) -> Result<()> {
        let pool = match &self.pool {
            Some(p) => p,
            None => return Ok(()),
        };

        let client = pool.get().await?;

        // Only insert if we have valid position
        if event.latitude == 0.0 && event.longitude == 0.0 {
            debug!("Skipping position insert for {} - no position data", event.icao);
            return Ok(());
        }

        client
            .execute(
                "INSERT INTO aircraft_positions (
                    time, icao_address, latitude, longitude,
                    altitude_ft, ground_speed_kts, heading_deg, vertical_rate_fpm,
                    squawk
                ) VALUES (
                    NOW(), $1, $2, $3, $4, $5, $6, $7, $8
                )",
                &[
                    &event.icao,
                    &event.latitude,
                    &event.longitude,
                    &event.altitude_ft,
                    &event.speed_kts,
                    &event.heading_deg,
                    &event.vertical_rate_fpm,
                    &event.squawk,
                ],
            )
            .await?;

        // Update aircraft_info if we have callsign
        if !event.callsign.is_empty() {
            client
                .execute(
                    "INSERT INTO aircraft_info (icao_address, callsign, last_seen)
                     VALUES ($1, $2, NOW())
                     ON CONFLICT (icao_address) DO UPDATE SET
                        callsign = EXCLUDED.callsign,
                        last_seen = NOW()",
                    &[&event.icao, &event.callsign],
                )
                .await?;
        }

        Ok(())
    }

    /// Update SDR device status
    pub async fn update_sdr_status(&self, status: &DeviceStatus) -> Result<()> {
        let pool = match &self.pool {
            Some(p) => p,
            None => return Ok(()),
        };

        let client = pool.get().await?;

        client
            .execute(
                "INSERT INTO sdr_status (
                    device_id, connected, sample_rate, center_freq, gain_db, last_heartbeat
                ) VALUES ($1, $2, $3, $4, $5, NOW())
                ON CONFLICT (device_id) DO UPDATE SET
                    connected = EXCLUDED.connected,
                    sample_rate = EXCLUDED.sample_rate,
                    center_freq = EXCLUDED.center_freq,
                    gain_db = EXCLUDED.gain_db,
                    last_heartbeat = NOW()",
                &[
                    &status.device_id,
                    &status.connected,
                    &(status.sample_rate as i32),
                    &(status.center_freq as i64),
                    &status.gain_db,
                ],
            )
            .await?;

        Ok(())
    }

    /// Get current aircraft list
    pub async fn get_current_aircraft(&self) -> Result<Vec<JsonValue>> {
        let pool = match &self.pool {
            Some(p) => p,
            None => return Ok(vec![]),
        };

        let client = pool.get().await?;

        let rows = client
            .query(
                "SELECT
                    icao_address as icao,
                    callsign,
                    latitude as lat,
                    longitude as lon,
                    altitude_ft as altitude,
                    ground_speed_kts as speed,
                    heading_deg as heading,
                    vertical_rate_fpm as vrate,
                    squawk,
                    last_seen as seen,
                    message_count as messages
                FROM current_aircraft
                ORDER BY last_seen DESC",
                &[],
            )
            .await?;

        let aircraft: Vec<JsonValue> = rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "icao": row.get::<_, Option<String>>("icao"),
                    "callsign": row.get::<_, Option<String>>("callsign"),
                    "lat": row.get::<_, Option<f64>>("lat"),
                    "lon": row.get::<_, Option<f64>>("lon"),
                    "altitude": row.get::<_, Option<i32>>("altitude"),
                    "speed": row.get::<_, Option<f32>>("speed"),
                    "heading": row.get::<_, Option<f32>>("heading"),
                    "vrate": row.get::<_, Option<i32>>("vrate"),
                    "squawk": row.get::<_, Option<String>>("squawk"),
                    "seen": row.get::<_, Option<chrono::DateTime<chrono::Utc>>>("seen")
                        .map(|dt| dt.to_rfc3339()),
                    "messages": row.get::<_, Option<i64>>("messages"),
                })
            })
            .collect();

        Ok(aircraft)
    }

    /// Get aircraft position trail
    pub async fn get_aircraft_trail(&self, icao: &str, minutes: i32) -> Result<Vec<JsonValue>> {
        let pool = match &self.pool {
            Some(p) => p,
            None => return Ok(vec![]),
        };

        let client = pool.get().await?;

        let rows = client
            .query(
                "SELECT
                    time,
                    latitude as lat,
                    longitude as lon,
                    altitude_ft as altitude
                FROM aircraft_positions
                WHERE icao_address = $1
                  AND time > NOW() - INTERVAL '1 minute' * $2
                  AND latitude IS NOT NULL
                  AND longitude IS NOT NULL
                ORDER BY time ASC",
                &[&icao, &minutes],
            )
            .await?;

        let trail: Vec<JsonValue> = rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "time": row.get::<_, chrono::DateTime<chrono::Utc>>("time").to_rfc3339(),
                    "lat": row.get::<_, f64>("lat"),
                    "lon": row.get::<_, f64>("lon"),
                    "altitude": row.get::<_, Option<i32>>("altitude"),
                })
            })
            .collect();

        Ok(trail)
    }

    /// Get current SDR status
    pub async fn get_sdr_status(&self) -> Result<JsonValue> {
        let pool = match &self.pool {
            Some(p) => p,
            None => {
                return Ok(serde_json::json!({
                    "connected": false,
                    "status": "no_database",
                    "error": "Database not available"
                }));
            }
        };

        let client = pool.get().await?;

        let row = client
            .query_opt(
                "SELECT
                    device_id,
                    connected,
                    sample_rate,
                    center_freq,
                    gain_db,
                    last_heartbeat,
                    messages_per_second,
                    CASE
                        WHEN connected AND last_heartbeat > NOW() - INTERVAL '30 seconds' THEN 'active'
                        WHEN last_heartbeat > NOW() - INTERVAL '5 minutes' THEN 'stale'
                        ELSE 'disconnected'
                    END as status
                FROM current_sdr_status
                ORDER BY last_heartbeat DESC
                LIMIT 1",
                &[],
            )
            .await?;

        match row {
            Some(row) => Ok(serde_json::json!({
                "device_id": row.get::<_, Option<String>>("device_id"),
                "connected": row.get::<_, Option<bool>>("connected").unwrap_or(false),
                "sample_rate": row.get::<_, Option<i32>>("sample_rate"),
                "center_freq": row.get::<_, Option<i64>>("center_freq"),
                "gain_db": row.get::<_, Option<f32>>("gain_db"),
                "last_heartbeat": row.get::<_, Option<chrono::DateTime<chrono::Utc>>>("last_heartbeat")
                    .map(|dt| dt.to_rfc3339()),
                "messages_per_second": row.get::<_, Option<f32>>("messages_per_second"),
                "status": row.get::<_, Option<String>>("status"),
            })),
            None => Ok(serde_json::json!({
                "connected": false,
                "status": "disconnected",
            })),
        }
    }
}
