-- Enable TimescaleDB extension
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- Aircraft information table (static data)
CREATE TABLE IF NOT EXISTS aircraft_info (
    icao_address VARCHAR(6) PRIMARY KEY,
    callsign VARCHAR(8),
    category VARCHAR(4),
    registration VARCHAR(10),
    aircraft_type VARCHAR(10),
    first_seen TIMESTAMPTZ DEFAULT NOW(),
    last_seen TIMESTAMPTZ DEFAULT NOW(),
    message_count BIGINT DEFAULT 1
);

-- Aircraft positions (time-series data)
CREATE TABLE IF NOT EXISTS aircraft_positions (
    time TIMESTAMPTZ NOT NULL,
    icao_address VARCHAR(6) NOT NULL,
    device_id VARCHAR(64),  -- Which SDR device received this message
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    altitude_ft INTEGER,
    ground_speed_kts REAL,
    heading_deg REAL,
    vertical_rate_fpm INTEGER,
    squawk VARCHAR(4),
    signal_strength_db REAL,
    raw_message BYTEA
);

-- Convert to hypertable for time-series optimization
SELECT create_hypertable('aircraft_positions', 'time',
    chunk_time_interval => INTERVAL '1 hour',
    if_not_exists => TRUE
);

-- Create indexes for common queries
CREATE INDEX IF NOT EXISTS idx_positions_icao ON aircraft_positions (icao_address, time DESC);
CREATE INDEX IF NOT EXISTS idx_positions_location ON aircraft_positions (latitude, longitude, time DESC);
CREATE INDEX IF NOT EXISTS idx_positions_device ON aircraft_positions (device_id, time DESC);

-- Enable compression for older data (compress chunks older than 1 day)
ALTER TABLE aircraft_positions SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'icao_address'
);

SELECT add_compression_policy('aircraft_positions', INTERVAL '1 day', if_not_exists => TRUE);

-- Create retention policy (keep 30 days of data)
SELECT add_retention_policy('aircraft_positions', INTERVAL '30 days', if_not_exists => TRUE);

-- Function to update aircraft_info on new message
CREATE OR REPLACE FUNCTION update_aircraft_info()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO aircraft_info (icao_address, last_seen, message_count)
    VALUES (NEW.icao_address, NEW.time, 1)
    ON CONFLICT (icao_address) DO UPDATE SET
        last_seen = NEW.time,
        message_count = aircraft_info.message_count + 1;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger to auto-update aircraft_info
DROP TRIGGER IF EXISTS trg_update_aircraft_info ON aircraft_positions;
CREATE TRIGGER trg_update_aircraft_info
    AFTER INSERT ON aircraft_positions
    FOR EACH ROW
    EXECUTE FUNCTION update_aircraft_info();

-- View for current aircraft state (most recent position per aircraft)
CREATE OR REPLACE VIEW current_aircraft AS
SELECT DISTINCT ON (p.icao_address)
    p.icao_address,
    i.callsign,
    i.category,
    p.latitude,
    p.longitude,
    p.altitude_ft,
    p.ground_speed_kts,
    p.heading_deg,
    p.vertical_rate_fpm,
    p.squawk,
    p.device_id,
    p.time as last_seen,
    i.message_count
FROM aircraft_positions p
LEFT JOIN aircraft_info i ON p.icao_address = i.icao_address
WHERE p.time > NOW() - INTERVAL '5 minutes'
ORDER BY p.icao_address, p.time DESC;

-- Function for NOTIFY on new position (for real-time updates)
CREATE OR REPLACE FUNCTION notify_new_position()
RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify('new_position', json_build_object(
        'icao_address', NEW.icao_address,
        'device_id', NEW.device_id,
        'latitude', NEW.latitude,
        'longitude', NEW.longitude,
        'altitude_ft', NEW.altitude_ft,
        'ground_speed_kts', NEW.ground_speed_kts,
        'heading_deg', NEW.heading_deg,
        'vertical_rate_fpm', NEW.vertical_rate_fpm,
        'time', NEW.time
    )::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_notify_position ON aircraft_positions;
CREATE TRIGGER trg_notify_position
    AFTER INSERT ON aircraft_positions
    FOR EACH ROW
    EXECUTE FUNCTION notify_new_position();

-- SDR device status table
CREATE TABLE IF NOT EXISTS sdr_status (
    id SERIAL PRIMARY KEY,
    device_id VARCHAR(64) NOT NULL UNIQUE,
    connected BOOLEAN DEFAULT FALSE,
    sample_rate INTEGER,
    center_freq BIGINT,
    gain_db REAL,
    ppm_error INTEGER,
    last_heartbeat TIMESTAMPTZ DEFAULT NOW(),
    messages_per_second REAL DEFAULT 0,
    error_message TEXT
);

-- Signal metrics table (time-series)
CREATE TABLE IF NOT EXISTS signal_metrics (
    time TIMESTAMPTZ NOT NULL,
    device_id VARCHAR(64) NOT NULL,
    signal_power_db REAL,
    noise_floor_db REAL,
    snr_db REAL,
    messages_decoded INTEGER DEFAULT 0,
    samples_processed BIGINT DEFAULT 0
);

-- Convert signal_metrics to hypertable
SELECT create_hypertable('signal_metrics', 'time',
    chunk_time_interval => INTERVAL '1 hour',
    if_not_exists => TRUE
);

-- Index for signal metrics queries
CREATE INDEX IF NOT EXISTS idx_signal_metrics_device ON signal_metrics (device_id, time DESC);

-- Retention policy for signal metrics (keep 7 days)
SELECT add_retention_policy('signal_metrics', INTERVAL '7 days', if_not_exists => TRUE);

-- View for current SDR status
CREATE OR REPLACE VIEW current_sdr_status AS
SELECT
    s.device_id,
    s.connected,
    s.sample_rate,
    s.center_freq,
    s.gain_db,
    s.last_heartbeat,
    s.messages_per_second,
    s.error_message,
    CASE
        WHEN s.last_heartbeat > NOW() - INTERVAL '10 seconds' THEN 'active'
        WHEN s.last_heartbeat > NOW() - INTERVAL '30 seconds' THEN 'stale'
        ELSE 'disconnected'
    END as status,
    m.signal_power_db,
    m.noise_floor_db,
    m.snr_db
FROM sdr_status s
LEFT JOIN LATERAL (
    SELECT signal_power_db, noise_floor_db, snr_db
    FROM signal_metrics
    WHERE device_id = s.device_id
    ORDER BY time DESC
    LIMIT 1
) m ON true;

-- Grant permissions
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO adsb;
GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA public TO adsb;
