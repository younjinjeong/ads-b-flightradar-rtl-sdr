//! Mode S preamble detection and frame extraction
//!
//! Mode S preamble pattern (at 2 MSPS = 0.5µs per sample):
//! Pulse at 0, 1, 3.5, 4.5 µs → samples 0, 2, 7, 9
//! Each pulse is 0.5µs = 1 sample wide
//!
//! Frame structure:
//! - Preamble: 8µs (16 samples)
//! - Data: 56 bits (short) or 112 bits (long) at 1µs per bit = 2 samples per bit

use super::MagnitudeTable;
use tracing::{debug, trace};

/// ADS-B/Mode S frame types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    Short,  // 56 bits (DF 0, 4, 5, 11)
    Long,   // 112 bits (DF 16, 17, 18, 19, 20, 21, 24)
}

/// Decoded Mode S frame
#[derive(Debug, Clone)]
pub struct Frame {
    pub frame_type: FrameType,
    pub data: Vec<u8>,  // Raw bytes (7 or 14 bytes)
    pub signal_level: u16,  // Signal strength
    pub timestamp_samples: u64,  // Sample offset when frame was detected
}

impl Frame {
    /// Get the Downlink Format (first 5 bits)
    pub fn df(&self) -> u8 {
        self.data[0] >> 3
    }

    /// Convert to hex string (like dump1090 output)
    pub fn to_hex(&self) -> String {
        self.data.iter().map(|b| format!("{:02X}", b)).collect()
    }
}

/// Mode S detector - finds preambles and extracts frames
pub struct ModeS {
    mag_table: MagnitudeTable,
    /// Minimum signal level to consider (noise floor threshold)
    min_signal: u16,
    /// Sample counter for timestamps
    sample_counter: u64,
    /// Statistics
    pub stats: DetectorStats,
    /// Debug: track signal levels for diagnostics
    debug_logged: bool,
    max_magnitude_seen: u16,
    /// Adaptive noise floor (moving average)
    noise_floor: u32,
    /// Noise floor sample count for moving average
    noise_samples: u64,
}

#[derive(Debug, Default)]
pub struct DetectorStats {
    pub samples_processed: u64,
    pub preambles_detected: u64,
    pub frames_decoded: u64,
    pub crc_errors: u64,
    pub short_frames: u64,
    pub long_frames: u64,
    pub corrected_frames: u64,
}

// Mode S preamble timing (in samples at 2 MSPS)
const PREAMBLE_SAMPLES: usize = 16;
const SHORT_FRAME_BITS: usize = 56;
const LONG_FRAME_BITS: usize = 112;
const SAMPLES_PER_BIT: usize = 2;

impl ModeS {
    pub fn new() -> Self {
        Self {
            mag_table: MagnitudeTable::new(),
            min_signal: 10,  // Very low threshold - will use adaptive detection
            sample_counter: 0,
            stats: DetectorStats::default(),
            debug_logged: false,
            max_magnitude_seen: 0,
            noise_floor: 0,
            noise_samples: 0,
        }
    }

    /// Set minimum signal threshold
    pub fn set_threshold(&mut self, threshold: u16) {
        self.min_signal = threshold;
    }

    /// Process a buffer of IQ samples and return detected frames
    pub fn process_buffer(&mut self, iq_data: &[u8]) -> Vec<Frame> {
        let num_samples = iq_data.len() / 2;
        if num_samples < PREAMBLE_SAMPLES + LONG_FRAME_BITS * SAMPLES_PER_BIT {
            return Vec::new();
        }

        // Convert to magnitude
        let mut magnitude = vec![0u16; num_samples];
        self.mag_table.compute_magnitudes(iq_data, &mut magnitude);

        // Calculate adaptive noise floor using moving average
        // Sample every 1000th value to save CPU
        let sample_step = 1000.min(num_samples / 100).max(1);
        let mut sum: u64 = 0;
        let mut count = 0u64;
        for i in (0..num_samples).step_by(sample_step) {
            sum += magnitude[i] as u64;
            count += 1;
        }
        if count > 0 {
            let buffer_avg = sum / count;
            // Exponential moving average: new_avg = 0.9 * old_avg + 0.1 * new_sample
            if self.noise_samples == 0 {
                self.noise_floor = buffer_avg as u32;
            } else {
                self.noise_floor = (self.noise_floor * 9 + buffer_avg as u32) / 10;
            }
            self.noise_samples += 1;
        }

        // Adaptive threshold: 4x noise floor, minimum 10
        // With noise floor of ~1, this gives threshold of ~10
        // Real ADS-B signals should be well above this
        let adaptive_threshold = (self.noise_floor * 4).max(10) as u16;

        // Track max magnitude for diagnostics (every ~10 buffers)
        if self.stats.samples_processed % (num_samples as u64 * 10) < num_samples as u64 {
            let max_in_buffer = magnitude.iter().cloned().max().unwrap_or(0);
            if max_in_buffer > self.max_magnitude_seen {
                self.max_magnitude_seen = max_in_buffer;
            }

            // Log signal levels periodically for debugging
            if !self.debug_logged || self.stats.samples_processed % 10_000_000 < num_samples as u64 {
                debug!(
                    "Signal levels: noise_floor={}, adaptive_threshold={}, max_buffer={}, max_ever={}",
                    self.noise_floor, adaptive_threshold, max_in_buffer, self.max_magnitude_seen
                );
                self.debug_logged = true;
            }
        }

        let mut frames = Vec::new();
        let mut i = 0;

        // Scan for preambles
        let scan_limit = num_samples - PREAMBLE_SAMPLES - LONG_FRAME_BITS * SAMPLES_PER_BIT;

        while i < scan_limit {
            if self.detect_preamble_adaptive(&magnitude, i, adaptive_threshold) {
                self.stats.preambles_detected += 1;

                // Try to decode frame
                if let Some(frame) = self.decode_frame(&magnitude, i) {
                    trace!(
                        "Frame detected at sample {}: DF={} hex={}",
                        self.sample_counter + i as u64,
                        frame.df(),
                        frame.to_hex()
                    );

                    self.stats.frames_decoded += 1;
                    match frame.frame_type {
                        FrameType::Short => self.stats.short_frames += 1,
                        FrameType::Long => self.stats.long_frames += 1,
                    }

                    // Skip past this frame
                    let skip = PREAMBLE_SAMPLES + match frame.frame_type {
                        FrameType::Short => SHORT_FRAME_BITS * SAMPLES_PER_BIT,
                        FrameType::Long => LONG_FRAME_BITS * SAMPLES_PER_BIT,
                    };
                    i += skip;
                    frames.push(frame);
                    continue;
                }
            }
            i += 1;
        }

        self.stats.samples_processed += num_samples as u64;
        self.sample_counter += num_samples as u64;

        frames
    }

    /// Detect Mode S preamble at given position
    /// Preamble: pulses at samples 0, 2, 7, 9 (at 2 MSPS)
    ///
    /// This uses dump1090-style detection which is more robust:
    /// - Check that pulses are above noise floor
    /// - Check relative pulse heights (all pulses should be similar)
    /// - Check that spaces are lower than pulses
    fn detect_preamble(&self, mag: &[u16], pos: usize) -> bool {
        if pos + 16 > mag.len() {
            return false;
        }

        // Get pulse magnitudes at expected positions
        // Mode S preamble at 2 MSPS: pulses at 0µs, 1µs, 3.5µs, 4.5µs
        // = samples 0, 2, 7, 9
        let p0 = mag[pos] as i32;
        let p1 = mag[pos + 2] as i32;
        let p2 = mag[pos + 7] as i32;
        let p3 = mag[pos + 9] as i32;

        // Get space (quiet period) magnitudes
        let s1 = mag[pos + 1] as i32;   // Between p0 and p1
        let s2 = mag[pos + 3] as i32;   // After p1
        let s3 = mag[pos + 4] as i32;
        let s4 = mag[pos + 5] as i32;
        let s5 = mag[pos + 6] as i32;   // Before p2
        let s6 = mag[pos + 8] as i32;   // Between p2 and p3
        let s7 = mag[pos + 10] as i32;  // After p3

        // Calculate sums for efficiency
        let pulse_sum = p0 + p1 + p2 + p3;
        let space_sum = s1 + s2 + s3 + s4 + s5 + s6 + s7;

        // dump1090 simplified preamble detection:
        // 1. Pulse sum should be significantly greater than space sum
        //    This is a relative check that works regardless of absolute signal level
        if pulse_sum <= space_sum * 2 {
            return false;
        }

        // 2. Minimum absolute signal - but very low threshold
        let high = p0.max(p1).max(p2).max(p3);
        if high < self.min_signal as i32 {
            return false;
        }

        // 3. All pulses should be reasonable (none should be noise-floor)
        let low_pulse = p0.min(p1).min(p2).min(p3);
        // At least half the max
        if low_pulse * 2 < high {
            return false;
        }

        // 4. Spaces should be notably lower than pulses
        let space_max = s1.max(s2).max(s3).max(s4).max(s5).max(s6).max(s7);
        // Space max should be less than 2/3 of pulse min
        if space_max * 3 > low_pulse * 2 {
            return false;
        }

        true
    }

    /// Detect Mode S preamble with adaptive threshold and correlation scoring
    /// Uses correlation-based detection for better weak signal performance
    fn detect_preamble_adaptive(&self, mag: &[u16], pos: usize, adaptive_threshold: u16) -> bool {
        if pos + 16 > mag.len() {
            return false;
        }

        // Get pulse magnitudes at expected positions
        // Mode S preamble at 2 MSPS: pulses at 0µs, 1µs, 3.5µs, 4.5µs
        // = samples 0, 2, 7, 9
        let p0 = mag[pos] as i32;
        let p1 = mag[pos + 2] as i32;
        let p2 = mag[pos + 7] as i32;
        let p3 = mag[pos + 9] as i32;

        // Get space (quiet period) magnitudes
        let s1 = mag[pos + 1] as i32;   // Between p0 and p1
        let s2 = mag[pos + 3] as i32;   // After p1
        let s3 = mag[pos + 4] as i32;
        let s4 = mag[pos + 5] as i32;
        let s5 = mag[pos + 6] as i32;   // Before p2
        let s6 = mag[pos + 8] as i32;   // Between p2 and p3
        let s7 = mag[pos + 10] as i32;  // After p3

        // === Correlation-based scoring ===
        // Expected pattern: [1, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, ...]
        // Pulse positions get +1, space positions get -1
        // Higher correlation = more likely a real preamble
        let correlation = (p0 + p1 + p2 + p3) - (s1 + s2 + s3 + s4 + s5 + s6 + s7);

        // Minimum correlation threshold (adaptive based on signal level)
        // Require correlation to be at least 3x the adaptive threshold
        // This is stricter to reject noise
        if correlation < (adaptive_threshold as i32 * 3) {
            return false;
        }

        // === Signal strength check ===
        let pulse_sum = p0 + p1 + p2 + p3;
        let space_sum = s1 + s2 + s3 + s4 + s5 + s6 + s7;

        // Pulse sum should be significantly greater than space sum (3x, stricter)
        if pulse_sum <= space_sum * 3 {
            return false;
        }

        // Minimum absolute signal using adaptive threshold
        let high = p0.max(p1).max(p2).max(p3);
        if high < adaptive_threshold as i32 {
            return false;
        }

        // === Pulse consistency check ===
        // All pulses should be reasonable (within 3x of each other)
        let low_pulse = p0.min(p1).min(p2).min(p3);
        if low_pulse * 3 < high {
            return false;
        }

        // === Space check ===
        // Spaces should be notably lower than pulses
        let space_max = s1.max(s2).max(s3).max(s4).max(s5).max(s6).max(s7);
        // Space max should be less than 2/3 of pulse min
        if space_max * 3 > low_pulse * 2 {
            return false;
        }

        // === Additional weak signal check ===
        // Check the "quiet zone" after preamble (samples 11-15)
        // These should also be relatively low
        if pos + 16 < mag.len() {
            let quiet_zone_avg = (mag[pos + 11] as i32 + mag[pos + 12] as i32 +
                                  mag[pos + 13] as i32 + mag[pos + 14] as i32 +
                                  mag[pos + 15] as i32) / 5;
            // Quiet zone should be below the average pulse level
            let pulse_avg = pulse_sum / 4;
            if quiet_zone_avg > pulse_avg {
                return false;
            }
        }

        true
    }

    /// Decode a frame starting at preamble position
    fn decode_frame(&mut self, mag: &[u16], preamble_pos: usize) -> Option<Frame> {
        let data_start = preamble_pos + PREAMBLE_SAMPLES;

        // Calculate signal level from preamble
        let signal_level = (mag[preamble_pos] as u32 + mag[preamble_pos + 2] as u32 +
                          mag[preamble_pos + 7] as u32 + mag[preamble_pos + 9] as u32) / 4;

        // Try long frame first (most ADS-B is DF17/18 = long)
        if data_start + LONG_FRAME_BITS * SAMPLES_PER_BIT <= mag.len() {
            let (bytes, confidence) = self.extract_bits_with_confidence(mag, data_start, LONG_FRAME_BITS);
            if self.verify_crc(&bytes) {
                return Some(Frame {
                    frame_type: FrameType::Long,
                    data: bytes,
                    signal_level: signal_level as u16,
                    timestamp_samples: self.sample_counter + preamble_pos as u64,
                });
            }

            // Try 1-bit error correction for long frames (DF17/18 are most valuable)
            if let Some(corrected) = self.try_single_bit_correction(&bytes, &confidence, LONG_FRAME_BITS) {
                self.stats.corrected_frames += 1;
                trace!("Corrected 1-bit error in long frame");
                return Some(Frame {
                    frame_type: FrameType::Long,
                    data: corrected,
                    signal_level: signal_level as u16,
                    timestamp_samples: self.sample_counter + preamble_pos as u64,
                });
            }
        }

        // Try short frame
        if data_start + SHORT_FRAME_BITS * SAMPLES_PER_BIT <= mag.len() {
            let bytes = self.extract_bits(mag, data_start, SHORT_FRAME_BITS);
            if self.verify_crc(&bytes) {
                return Some(Frame {
                    frame_type: FrameType::Short,
                    data: bytes,
                    signal_level: signal_level as u16,
                    timestamp_samples: self.sample_counter + preamble_pos as u64,
                });
            }
        }

        // Log CRC error details for diagnostics (sample every 10th error to avoid spam)
        self.stats.crc_errors += 1;
        if self.stats.crc_errors <= 10 || self.stats.crc_errors % 50 == 0 {
            if data_start + LONG_FRAME_BITS * SAMPLES_PER_BIT <= mag.len() {
                let (bytes, confidence) = self.extract_bits_with_confidence(mag, data_start, LONG_FRAME_BITS);
                let df = (bytes[0] >> 3) & 0x1F;
                let avg_confidence: i32 = confidence.iter().sum::<i32>() / confidence.len() as i32;
                let min_confidence = *confidence.iter().min().unwrap_or(&0);
                let low_confidence_bits = confidence.iter().filter(|&&c| c.abs() < 5).count();

                debug!(
                    "CRC error #{}: DF={} signal={} avg_conf={} min_conf={} low_bits={} hex={}",
                    self.stats.crc_errors,
                    df,
                    signal_level,
                    avg_confidence,
                    min_confidence,
                    low_confidence_bits,
                    hex::encode(&bytes)
                );
            }
        }
        None
    }

    /// Extract bits from magnitude samples using Manchester decoding
    /// Each bit is 2 samples: high-low = 1, low-high = 0
    ///
    /// This uses dump1090-style bit extraction which is more robust:
    /// - Compares first half vs second half of each bit period
    /// - Uses the difference to determine confidence
    fn extract_bits(&self, mag: &[u16], start: usize, num_bits: usize) -> Vec<u8> {
        let num_bytes = (num_bits + 7) / 8;
        let mut bytes = vec![0u8; num_bytes];

        for bit_idx in 0..num_bits {
            let sample_pos = start + bit_idx * SAMPLES_PER_BIT;
            let first_half = mag[sample_pos] as i32;
            let second_half = mag[sample_pos + 1] as i32;

            // Manchester: first > second = 1, first < second = 0
            if first_half > second_half {
                let byte_idx = bit_idx / 8;
                let bit_pos = 7 - (bit_idx % 8);
                bytes[byte_idx] |= 1 << bit_pos;
            }
        }

        bytes
    }

    /// Extract bits with confidence values for error correction
    /// Returns (bytes, confidence) where confidence[i] is how certain we are about bit i
    fn extract_bits_with_confidence(&self, mag: &[u16], start: usize, num_bits: usize) -> (Vec<u8>, Vec<i32>) {
        let num_bytes = (num_bits + 7) / 8;
        let mut bytes = vec![0u8; num_bytes];
        let mut confidence = vec![0i32; num_bits];

        for bit_idx in 0..num_bits {
            let sample_pos = start + bit_idx * SAMPLES_PER_BIT;
            let first_half = mag[sample_pos] as i32;
            let second_half = mag[sample_pos + 1] as i32;

            // Confidence is the difference between halves
            let diff = first_half - second_half;
            confidence[bit_idx] = diff.abs();

            // Manchester: first > second = 1, first < second = 0
            if diff > 0 {
                let byte_idx = bit_idx / 8;
                let bit_pos = 7 - (bit_idx % 8);
                bytes[byte_idx] |= 1 << bit_pos;
            }
        }

        (bytes, confidence)
    }

    /// Try to correct single bit errors by flipping low-confidence bits
    /// This is based on dump1090's error correction approach
    fn try_single_bit_correction(&self, bytes: &[u8], confidence: &[i32], num_bits: usize) -> Option<Vec<u8>> {
        // Find the bits with lowest confidence (most likely to be errors)
        // Sort indices by confidence, try flipping lowest confidence bits first
        let mut indices: Vec<usize> = (0..num_bits).collect();
        indices.sort_by_key(|&i| confidence[i]);

        // Try flipping each bit (all 112 bits for thorough correction)
        for bit_idx in 0..num_bits {
            let mut test_bytes = bytes.to_vec();
            let byte_idx = bit_idx / 8;
            let bit_pos = 7 - (bit_idx % 8);
            test_bytes[byte_idx] ^= 1 << bit_pos;

            if self.verify_crc(&test_bytes) {
                // Check if the DF is valid (11, 17, or 18)
                let df = (test_bytes[0] >> 3) & 0x1F;
                if df == 11 || df == 17 || df == 18 {
                    return Some(test_bytes);
                }
            }
        }

        // For weak signals, try 2-bit correction on the lowest confidence bits
        // This is more expensive but can recover more frames
        let max_2bit = 30.min(num_bits); // Top 30 lowest confidence bits
        for i in 0..max_2bit {
            for j in (i+1)..max_2bit {
                let bit_idx1 = indices[i];
                let bit_idx2 = indices[j];

                let mut test_bytes = bytes.to_vec();

                let byte_idx1 = bit_idx1 / 8;
                let bit_pos1 = 7 - (bit_idx1 % 8);
                test_bytes[byte_idx1] ^= 1 << bit_pos1;

                let byte_idx2 = bit_idx2 / 8;
                let bit_pos2 = 7 - (bit_idx2 % 8);
                test_bytes[byte_idx2] ^= 1 << bit_pos2;

                if self.verify_crc(&test_bytes) {
                    // Check if the DF is valid (11, 17, or 18)
                    let df = (test_bytes[0] >> 3) & 0x1F;
                    if df == 11 || df == 17 || df == 18 {
                        return Some(test_bytes);
                    }
                }
            }
        }

        None
    }

    /// Verify CRC-24 checksum
    fn verify_crc(&self, data: &[u8]) -> bool {
        // Use the same CRC from our adsb module
        crate::adsb::verify_crc(data)
    }

    /// Get current statistics
    pub fn get_stats(&self) -> &DetectorStats {
        &self.stats
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.stats = DetectorStats::default();
    }

    /// Get current noise floor value
    pub fn get_noise_floor(&self) -> u32 {
        self.noise_floor
    }

    /// Get maximum magnitude seen
    pub fn get_max_magnitude(&self) -> u16 {
        self.max_magnitude_seen
    }
}

impl Default for ModeS {
    fn default() -> Self {
        Self::new()
    }
}
