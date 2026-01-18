//! Decoder runner - spawns rtl_adsb.exe subprocess and reads output

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Decoder runner that manages rtl_adsb.exe subprocess
pub struct DecoderRunner {
    rtl_adsb_path: String,
    device_index: u32,
    gain_db: f32,
    ppm_error: i32,
    running: Arc<AtomicBool>,
    messages_received: Arc<AtomicU64>,
    parse_errors: Arc<AtomicU64>,
}

impl DecoderRunner {
    pub fn new(
        rtl_adsb_path: &Path,
        device_index: u32,
        gain_db: f32,
        ppm_error: i32,
    ) -> Self {
        Self {
            rtl_adsb_path: rtl_adsb_path.to_string_lossy().to_string(),
            device_index,
            gain_db,
            ppm_error,
            running: Arc::new(AtomicBool::new(false)),
            messages_received: Arc::new(AtomicU64::new(0)),
            parse_errors: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Start the decoder and send raw ADS-B bytes to the channel
    pub async fn run(&self, tx: mpsc::Sender<Vec<u8>>) -> Result<()> {
        info!(
            "Starting rtl_adsb: {} -d {} -g {} -p {}",
            self.rtl_adsb_path, self.device_index, self.gain_db, self.ppm_error
        );

        let mut child = Command::new(&self.rtl_adsb_path)
            .args([
                "-d",
                &self.device_index.to_string(),
                "-g",
                &self.gain_db.to_string(),
                "-p",
                &self.ppm_error.to_string(),
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn rtl_adsb.exe")?;

        self.running.store(true, Ordering::SeqCst);

        let stdout = child
            .stdout
            .take()
            .context("Failed to capture rtl_adsb stdout")?;

        let stderr = child
            .stderr
            .take()
            .context("Failed to capture rtl_adsb stderr")?;

        // Spawn task to read stderr for error/info messages
        let stderr_handle = tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.is_empty() {
                    // rtl_adsb outputs info to stderr, not just errors
                    info!("rtl_adsb: {}", line);
                }
            }
        });

        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        let messages_received = self.messages_received.clone();
        let parse_errors = self.parse_errors.clone();
        let running = self.running.clone();

        info!("Waiting for ADS-B messages from rtl_adsb...");
        let mut first_message = true;

        // Read lines from rtl_adsb output
        while running.load(Ordering::SeqCst) {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if let Some(msg) = parse_hex_line(&line) {
                        if first_message {
                            info!("First ADS-B message received! Decoder is working.");
                            first_message = false;
                        }
                        messages_received.fetch_add(1, Ordering::Relaxed);
                        if tx.send(msg).await.is_err() {
                            warn!("Channel closed, stopping decoder");
                            break;
                        }
                    } else if line.starts_with('*') {
                        // Failed to parse a message line
                        parse_errors.fetch_add(1, Ordering::Relaxed);
                        debug!("Failed to parse line: {}", line);
                    }
                    // Ignore non-message lines (e.g., rtl_adsb startup messages)
                }
                Ok(None) => {
                    info!("rtl_adsb stdout closed");
                    break;
                }
                Err(e) => {
                    error!("Error reading rtl_adsb output: {}", e);
                    break;
                }
            }
        }

        self.running.store(false, Ordering::SeqCst);

        // Try to kill the process if still running
        let _ = child.kill().await;

        // Wait for stderr reader to finish to capture all error messages
        let _ = stderr_handle.await;

        info!(
            "Decoder stopped. Messages: {}, Parse errors: {}",
            self.messages_received.load(Ordering::Relaxed),
            self.parse_errors.load(Ordering::Relaxed)
        );

        Ok(())
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn messages_received(&self) -> u64 {
        self.messages_received.load(Ordering::Relaxed)
    }

    pub fn parse_errors(&self) -> u64 {
        self.parse_errors.load(Ordering::Relaxed)
    }
}

/// Parse rtl_adsb output line format: *<hex_bytes>;\r\n
/// Returns the raw message bytes if valid
fn parse_hex_line(line: &str) -> Option<Vec<u8>> {
    let line = line.trim();

    // Must start with '*'
    if !line.starts_with('*') {
        return None;
    }

    // Find the semicolon
    let end_idx = line.find(';')?;
    let hex_str = &line[1..end_idx];

    // Valid lengths: 14 hex chars (7 bytes) or 28 hex chars (14 bytes)
    if hex_str.len() != 14 && hex_str.len() != 28 {
        return None;
    }

    // Parse hex to bytes
    hex::decode(hex_str).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_line_short() {
        let line = "*8D4840D6202CC371C32CE0576098;";
        let result = parse_hex_line(line);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 14);
    }

    #[test]
    fn test_parse_hex_line_with_crlf() {
        let line = "*8D4840D6202CC371C32CE0576098;\r\n";
        let result = parse_hex_line(line);
        assert!(result.is_some());
    }

    #[test]
    fn test_parse_hex_line_short_msg() {
        let line = "*02E197B2F3F9A1;";
        let result = parse_hex_line(line);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 7);
    }

    #[test]
    fn test_parse_hex_line_invalid() {
        assert!(parse_hex_line("not a message").is_none());
        assert!(parse_hex_line("*invalid;").is_none());
        assert!(parse_hex_line("*12345;").is_none()); // too short
    }
}
