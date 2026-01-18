-- Migration: Add device_id to aircraft_positions
-- This tracks which SDR device received each message (for multi-device support)

-- Add device_id column to aircraft_positions
ALTER TABLE aircraft_positions ADD COLUMN IF NOT EXISTS device_id VARCHAR(64);

-- Index for device-specific queries
CREATE INDEX IF NOT EXISTS idx_positions_device ON aircraft_positions (device_id, time DESC);

-- Update notify trigger to include device_id
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

-- Update current_aircraft view to include device_id
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
