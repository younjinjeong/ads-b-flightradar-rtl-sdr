#!/bin/bash
set -e

echo "=========================================="
echo "   Building ADS-B Tracker Services"
echo "=========================================="

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

# Build Rust DSP Processor
echo ""
echo "[1/3] Building Rust DSP Processor..."
docker build -t adsb-tracker/rust-dsp:latest -f services/rust-dsp/Dockerfile .

# Build WebSocket Gateway
echo ""
echo "[2/3] Building WebSocket Gateway..."
docker build -t adsb-tracker/websocket-gateway:latest -f services/websocket-gateway/Dockerfile .

# Build RTL-SDR Capture (optional - requires librtlsdr)
echo ""
echo "[3/3] Building RTL-SDR Capture..."
docker build -t adsb-tracker/rtl-sdr-capture:latest -f services/rtl-sdr-capture/Dockerfile . || {
    echo "Warning: RTL-SDR capture build failed (may need librtlsdr dependencies)"
}

echo ""
echo "=========================================="
echo "   Build Complete!"
echo "=========================================="
echo ""
echo "Images built:"
docker images | grep adsb-tracker
