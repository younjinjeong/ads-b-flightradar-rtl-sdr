//! RTL-SDR capture via rtl_sdr.exe process
//!
//! Spawns rtl_sdr.exe to capture raw IQ samples at 2 MSPS,
//! then processes them through our Rust Mode S decoder.

use anyhow::{Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use std::io::{BufRead, Read};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use super::detect::{Frame, ModeS};

/// Query RTL-SDR device serial number by device index
/// Parses the output of rtl_sdr -d N to extract the serial number
pub fn query_device_serial(rtl_sdr_path: &str, device_index: u32) -> Option<String> {
    // Run rtl_sdr briefly to get device info from stderr
    // The device info is printed when rtl_sdr starts
    let mut cmd = Command::new(rtl_sdr_path);
    cmd.arg("-d").arg(device_index.to_string())
       .arg("-f").arg("1090000000")
       .arg("-s").arg("2000000")
       .arg("-n").arg("1")  // Just read 1 sample then exit
       .arg("-")
       .stdout(Stdio::null())
       .stderr(Stdio::piped());

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to query device serial: {}", e);
            return None;
        }
    };

    // Read stderr for device info
    let stderr = match child.stderr {
        Some(s) => s,
        None => return None,
    };

    let reader = std::io::BufReader::new(stderr);
    let mut serial: Option<String> = None;

    for line in reader.lines().map_while(Result::ok) {
        // Look for device info line like:
        // "  0:  Realtek, RTL2838UHIDIR, SN: 00000001"
        // or "Found 1 device(s):" followed by device listing
        if line.contains("SN:") {
            if let Some(sn_start) = line.find("SN:") {
                let sn_part = &line[sn_start + 3..].trim();
                // Extract serial until next space or end of line
                let sn = sn_part.split_whitespace().next().unwrap_or("");
                if !sn.is_empty() && sn.chars().all(|c| c.is_alphanumeric()) {
                    serial = Some(sn.to_string());
                    break;
                }
            }
        }
    }

    // If we didn't find a clean serial, try another pattern
    // Sometimes the output shows device index and serial differently
    serial
}

/// Sanitize a string to only contain printable ASCII characters
fn sanitize_string(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .collect::<String>()
        .trim()
        .to_string()
}

/// Generate a hash-based device ID from manufacturer and product strings
fn generate_device_hash(manufacturer: &Option<String>, product: &Option<String>, device_index: u32) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    manufacturer.as_deref().unwrap_or("Unknown").hash(&mut hasher);
    product.as_deref().unwrap_or("RTL-SDR").hash(&mut hasher);
    device_index.hash(&mut hasher);
    let hash = hasher.finish();
    format!("{:08X}", hash as u32)
}

/// Query device info and return (manufacturer, product, serial)
/// If the serial contains non-printable characters, a hash-based ID is generated instead.
pub fn query_device_info(rtl_sdr_path: &str, device_index: u32) -> (Option<String>, Option<String>, Option<String>) {
    let mut cmd = Command::new(rtl_sdr_path);
    cmd.arg("-d").arg(device_index.to_string())
       .arg("-f").arg("1090000000")
       .arg("-s").arg("2000000")
       .arg("-n").arg("1")
       .arg("-")
       .stdout(Stdio::null())
       .stderr(Stdio::piped());

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to query device info: {}", e);
            return (None, None, None);
        }
    };

    let stderr = match child.stderr {
        Some(s) => s,
        None => return (None, None, None),
    };

    let reader = std::io::BufReader::new(stderr);
    let mut manufacturer: Option<String> = None;
    let mut product: Option<String> = None;
    let mut raw_serial: Option<String> = None;

    for line in reader.lines().map_while(Result::ok) {
        // Parse device listing line like:
        // "  0:  Realtek, RTL2838UHIDIR, SN: 00000001"
        let trimmed = line.trim();
        if trimmed.starts_with(&format!("{}:", device_index)) {
            // Format: "INDEX:  MANUFACTURER, PRODUCT, SN: SERIAL"
            let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
            if parts.len() == 2 {
                let info = parts[1].trim();
                let fields: Vec<&str> = info.split(',').collect();
                if !fields.is_empty() {
                    let mfr = sanitize_string(fields[0]);
                    if !mfr.is_empty() {
                        manufacturer = Some(mfr);
                    }
                }
                if fields.len() >= 2 {
                    let prd = sanitize_string(fields[1]);
                    if !prd.is_empty() {
                        product = Some(prd);
                    }
                }
                if fields.len() >= 3 {
                    let sn_part = fields[2].trim();
                    if let Some(sn) = sn_part.strip_prefix("SN:") {
                        raw_serial = Some(sn.trim().to_string());
                    }
                }
            }
        }
        // Also check "Using device" line
        if trimmed.starts_with("Using device") {
            // "Using device 0: Generic RTL2832U"
            if let Some(name_start) = trimmed.find(':') {
                let name = trimmed[name_start + 1..].trim();
                if product.is_none() && !name.is_empty() {
                    product = Some(sanitize_string(name));
                }
            }
        }
    }

    // Process the serial: sanitize and validate
    let serial = raw_serial.map(|s| {
        let sanitized = sanitize_string(&s);
        // If serial is empty, only whitespace, or the default "00000001", generate a hash instead
        if sanitized.is_empty() || sanitized == "00000001" {
            info!("Device serial '{}' is default/empty, generating hash-based ID", s);
            generate_device_hash(&manufacturer, &product, device_index)
        } else {
            sanitized
        }
    });

    (manufacturer, product, serial)
}

/// RTL-SDR configuration
#[derive(Clone)]
pub struct SdrConfig {
    pub device_index: u32,
    pub center_freq: u32,
    pub sample_rate: u32,
    pub gain: i32,           // Gain in tenths of dB (e.g., 496 = 49.6 dB)
    pub ppm_error: i32,
    pub rtl_sdr_path: String,
}

impl Default for SdrConfig {
    fn default() -> Self {
        Self {
            device_index: 0,
            center_freq: 1_090_000_000, // 1090 MHz for ADS-B
            sample_rate: 2_000_000,      // 2 MSPS (required for Mode S timing)
            gain: 496,                   // 49.6 dB
            ppm_error: 0,
            rtl_sdr_path: "rtl_sdr".to_string(),
        }
    }
}

/// Statistics for SDR capture (atomic for thread-safe access)
#[derive(Debug, Default)]
pub struct CaptureStats {
    pub samples_captured: AtomicU64,
    pub buffers_processed: AtomicU64,
    pub frames_detected: AtomicU64,
    pub preambles_detected: AtomicU64,
    pub crc_errors: AtomicU64,
    pub corrected_frames: AtomicU64,
    pub noise_floor: std::sync::atomic::AtomicU32,
    pub peak_signal: std::sync::atomic::AtomicU32,
}

impl CaptureStats {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

/// RTL-SDR capture controller
pub struct SdrCapture {
    config: SdrConfig,
    running: Arc<AtomicBool>,
    stats: Arc<CaptureStats>,
}

impl SdrCapture {
    pub fn new(config: SdrConfig) -> Self {
        Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            stats: CaptureStats::new(),
        }
    }

    /// Start capturing and return a receiver for decoded frames
    pub fn start(&self) -> Result<Receiver<Frame>> {
        info!("===========================================");
        info!("  Starting RTL-SDR Raw IQ Capture");
        info!("===========================================");
        info!("  Device index: {}", self.config.device_index);
        info!("  Center frequency: {} MHz", self.config.center_freq / 1_000_000);
        info!("  Sample rate: {} MSPS", self.config.sample_rate / 1_000_000);
        info!("  Gain: {:.1} dB", self.config.gain as f32 / 10.0);
        info!("  rtl_sdr path: {}", self.config.rtl_sdr_path);

        // Create channel for decoded frames
        let (frame_tx, frame_rx) = bounded::<Frame>(1000);

        // Clone for thread
        let config = self.config.clone();
        let running = self.running.clone();
        let stats = self.stats.clone();

        running.store(true, Ordering::SeqCst);

        // Spawn capture thread
        thread::Builder::new()
            .name("sdr-capture".to_string())
            .spawn(move || {
                if let Err(e) = run_capture(config, running, stats, frame_tx) {
                    error!("SDR capture error: {}", e);
                }
            })
            .context("Failed to spawn capture thread")?;

        Ok(frame_rx)
    }

    /// Stop capturing
    pub fn stop(&self) {
        info!("Stopping RTL-SDR capture...");
        self.running.store(false, Ordering::SeqCst);
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get statistics
    pub fn stats(&self) -> &Arc<CaptureStats> {
        &self.stats
    }
}

/// Main capture loop (runs in dedicated thread)
fn run_capture(
    config: SdrConfig,
    running: Arc<AtomicBool>,
    stats: Arc<CaptureStats>,
    frame_tx: Sender<Frame>,
) -> Result<()> {
    info!("Starting rtl_sdr process for raw IQ capture...");

    // Build rtl_sdr command:
    // rtl_sdr -d <device> -f <freq> -s <rate> -g <gain> -p <ppm> -
    // The "-" at the end means output to stdout
    let mut cmd = Command::new(&config.rtl_sdr_path);
    cmd.arg("-d").arg(config.device_index.to_string())
       .arg("-f").arg(config.center_freq.to_string())
       .arg("-s").arg(config.sample_rate.to_string())
       .arg("-g").arg((config.gain as f32 / 10.0).to_string());

    if config.ppm_error != 0 {
        cmd.arg("-p").arg(config.ppm_error.to_string());
    }

    // Output to stdout (continuous mode)
    cmd.arg("-");

    cmd.stdout(Stdio::piped())
       .stderr(Stdio::piped());

    info!("Executing: {:?}", cmd);

    let mut child = cmd.spawn()
        .context("Failed to spawn rtl_sdr. Make sure rtl_sdr.exe is installed and in PATH")?;

    let mut stdout = child.stdout.take()
        .context("Failed to capture rtl_sdr stdout")?;

    // Spawn stderr reader for logging
    if let Some(stderr) = child.stderr.take() {
        thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stderr);
            let mut line = String::new();
            while std::io::BufRead::read_line(&mut reader, &mut line).unwrap_or(0) > 0 {
                if !line.trim().is_empty() {
                    info!("[rtl_sdr] {}", line.trim());
                }
                line.clear();
            }
        });
    }

    info!("===========================================");
    info!("  LIVE IQ CAPTURE STARTED!");
    info!("  Receiving raw IQ samples at 1090 MHz");
    info!("  Processing with dump1090-style decoder");
    info!("===========================================");

    // Create Mode S detector
    let mut detector = ModeS::new();

    // Buffer for reading IQ samples
    // Process in chunks of 256K samples (512KB)
    const BUFFER_SIZE: usize = 256 * 1024 * 2; // * 2 for I and Q bytes
    let mut buffer = vec![0u8; BUFFER_SIZE];

    let mut last_stats_time = Instant::now();
    let mut last_sample_count = 0u64;
    let mut first_data = true;

    // Main capture loop
    while running.load(Ordering::SeqCst) {
        // Read a chunk of IQ samples
        match stdout.read(&mut buffer) {
            Ok(0) => {
                warn!("rtl_sdr stdout closed (EOF)");
                break;
            }
            Ok(n_read) => {
                if first_data {
                    info!("First IQ data received! ({} bytes)", n_read);
                    first_data = false;
                }

                let samples = n_read / 2;
                stats.samples_captured.fetch_add(samples as u64, Ordering::Relaxed);
                stats.buffers_processed.fetch_add(1, Ordering::Relaxed);

                // Process buffer through Mode S detector
                let frames = detector.process_buffer(&buffer[..n_read]);

                for frame in frames {
                    stats.frames_detected.fetch_add(1, Ordering::Relaxed);

                    // Log frame detection with prominent formatting
                    info!(
                        ">>> FRAME: DF={:02} | {} bytes | signal={} | *{};",
                        frame.df(),
                        frame.data.len(),
                        frame.signal_level,
                        frame.to_hex()
                    );

                    // Send to channel (non-blocking)
                    if frame_tx.try_send(frame).is_err() {
                        debug!("Frame channel full, dropping frame");
                    }
                }

                // Update stats from detector
                stats.preambles_detected.store(
                    detector.stats.preambles_detected,
                    Ordering::Relaxed
                );
                stats.crc_errors.store(
                    detector.stats.crc_errors,
                    Ordering::Relaxed
                );
                stats.corrected_frames.store(
                    detector.stats.corrected_frames,
                    Ordering::Relaxed
                );
                stats.noise_floor.store(
                    detector.get_noise_floor(),
                    Ordering::Relaxed
                );
                stats.peak_signal.store(
                    detector.get_max_magnitude() as u32,
                    Ordering::Relaxed
                );

                // Periodic stats logging (every 5 seconds)
                if last_stats_time.elapsed() >= Duration::from_secs(5) {
                    let current_samples = stats.samples_captured.load(Ordering::Relaxed);
                    let samples_delta = current_samples - last_sample_count;
                    let elapsed = last_stats_time.elapsed().as_secs_f32();
                    let sample_rate = samples_delta as f32 / elapsed;

                    info!(
                        "[SDR Stats] Rate: {:.2} MSPS | Preambles: {} | Frames: {} (corrected: {}) | CRC errors: {}",
                        sample_rate / 1_000_000.0,
                        detector.stats.preambles_detected,
                        detector.stats.frames_decoded,
                        detector.stats.corrected_frames,
                        detector.stats.crc_errors
                    );

                    last_stats_time = Instant::now();
                    last_sample_count = current_samples;
                }
            }
            Err(e) => {
                error!("Error reading from rtl_sdr: {}", e);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }

    // Kill the rtl_sdr process
    let _ = child.kill();

    info!("RTL-SDR capture stopped");
    info!(
        "Final stats: Samples={}, Preambles={}, Frames={} (corrected: {}), CRC errors={}",
        stats.samples_captured.load(Ordering::Relaxed),
        detector.stats.preambles_detected,
        detector.stats.frames_decoded,
        detector.stats.corrected_frames,
        detector.stats.crc_errors
    );

    Ok(())
}

impl Drop for SdrCapture {
    fn drop(&mut self) {
        self.stop();
    }
}
