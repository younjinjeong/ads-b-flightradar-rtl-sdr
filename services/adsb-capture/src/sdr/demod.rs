//! Magnitude computation for IQ samples
//!
//! RTL-SDR outputs 8-bit unsigned IQ samples (I, Q pairs).
//! We need to convert them to magnitude for signal detection.

/// Pre-computed magnitude lookup table for fast IQ → magnitude conversion
/// Index: (I << 8) | Q where I, Q are 0-255
pub struct MagnitudeTable {
    table: Vec<u16>,
}

impl MagnitudeTable {
    /// Create a new magnitude lookup table
    /// Uses the approximation: mag ≈ max(|I|, |Q|) + 0.4 * min(|I|, |Q|)
    /// This is faster than sqrt and good enough for signal detection
    pub fn new() -> Self {
        let mut table = vec![0u16; 256 * 256];

        for i in 0..256u32 {
            for q in 0..256u32 {
                // Convert from unsigned (0-255) to signed (-127 to 128)
                let si = (i as i32) - 127;
                let sq = (q as i32) - 127;

                // Compute magnitude using the fast approximation
                let ai = si.abs() as u32;
                let aq = sq.abs() as u32;

                // mag ≈ max + 0.4 * min (scaled to preserve precision)
                let mag = if ai > aq {
                    (ai << 8) + (aq * 102) // 102/256 ≈ 0.4
                } else {
                    (aq << 8) + (ai * 102)
                };

                table[(i * 256 + q) as usize] = (mag >> 8) as u16;
            }
        }

        Self { table }
    }

    /// Convert IQ sample pair to magnitude
    #[inline(always)]
    pub fn magnitude(&self, i: u8, q: u8) -> u16 {
        self.table[((i as usize) << 8) | (q as usize)]
    }

    /// Convert a buffer of IQ samples to magnitudes
    /// Input: pairs of (I, Q) bytes
    /// Output: magnitude values
    pub fn compute_magnitudes(&self, iq_data: &[u8], output: &mut [u16]) {
        let pairs = iq_data.len() / 2;
        for i in 0..pairs.min(output.len()) {
            output[i] = self.magnitude(iq_data[i * 2], iq_data[i * 2 + 1]);
        }
    }
}

impl Default for MagnitudeTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magnitude_table() {
        let table = MagnitudeTable::new();

        // Center (127, 127) should give near-zero magnitude
        let mag_center = table.magnitude(127, 127);
        assert!(mag_center < 10, "Center should be near zero");

        // Max positive I (255, 127) should give high magnitude
        let mag_high_i = table.magnitude(255, 127);
        assert!(mag_high_i > 100, "High I should give high magnitude");

        // Max positive Q (127, 255)
        let mag_high_q = table.magnitude(127, 255);
        assert!(mag_high_q > 100, "High Q should give high magnitude");
    }
}
