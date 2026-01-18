# ADS-B Flight Tracker

A real-time ADS-B flight tracking system built with RTL-SDR hardware, Rust signal processing, and Kubernetes orchestration.

## Architecture

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Kubernetes Cluster                                  │
│                                                                             │
│  ┌──────────────┐    gRPC     ┌──────────────┐    SQL     ┌──────────────┐ │
│  │  RTL-SDR     │ ─────────▶  │  Rust DSP    │ ─────────▶ │  TimescaleDB │ │
│  │  Capture     │             │  Processor   │            │              │ │
│  │  (C)         │             │  (FFT/Decode)│            │  (PostgreSQL)│ │
│  └──────────────┘             └──────────────┘            └──────────────┘ │
│        │                                                         │         │
│        │ USB                                                     │         │
│        ▼                                                         ▼         │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                         WebSocket Gateway (Rust/Axum)               │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                    │                                       │
└────────────────────────────────────│───────────────────────────────────────┘
                                     │ WebSocket
                                     ▼
                          ┌─────────────────────┐
                          │   Browser Client    │
                          │   (Leaflet.js)      │
                          └─────────────────────┘
```

## Components

| Service           | Technology         | Description                          |
| ----------------- | ------------------ | ------------------------------------ |
| RTL-SDR Capture   | C + librtlsdr      | Interfaces with USB SDR hardware     |
| Rust DSP          | Rust + rustfft     | FFT processing & ADS-B decoding      |
| TimescaleDB       | PostgreSQL         | Time-series database for flight data |
| WebSocket Gateway | Rust + Axum        | Real-time data streaming to browsers |
| Frontend          | Vanilla JS + Leaflet | Interactive flight map             |

## Features

- **Real-time tracking**: Aircraft positions update via WebSocket
- **FFT analysis**: Signal processing using Fast Fourier Transform (푸리에 변환)
- **ADS-B decoding**: Full Mode-S message decoding (position, velocity, callsign)
- **Time-series storage**: Efficient data storage with automatic compression
- **Interactive map**: Dark-themed map with aircraft icons, trails, and info panels
- **Kubernetes native**: Deploys to Docker Desktop Kubernetes

## Prerequisites

- Docker Desktop with Kubernetes enabled
- RTL-SDR USB device (for live capture)
- Zadig driver (Windows) or librtlsdr (Linux)

## Quick Start

### Windows (PowerShell)

```powershell
# Build and deploy everything
.\scripts\deploy.ps1 -All

# Check status
.\scripts\deploy.ps1 -Status

# View logs
.\scripts\deploy.ps1 -Logs

# Delete deployment
.\scripts\deploy.ps1 -Delete
```

### Linux/macOS (Bash)

```bash
# Build images
./scripts/build-all.sh

# Deploy to Kubernetes
./scripts/deploy.sh

# Check status
kubectl -n adsb-tracker get pods
```

### Access the Web UI

Open: **http://localhost:30888**

## Configuration

### Environment Variables

| Variable          | Default      | Description                            |
| ----------------- | ------------ | -------------------------------------- |
| `SDR_SAMPLE_RATE` | 2400000      | Sample rate in Hz                      |
| `SDR_CENTER_FREQ` | 1090000000   | Center frequency (1090 MHz for ADS-B)  |
| `SDR_GAIN`        | 49.6         | Gain in dB (0 for auto)                |
| `GRPC_PORT`       | 50051        | gRPC server port                       |
| `WS_PORT`         | 8888         | WebSocket/HTTP port                    |
| `DB_HOST`         | timescaledb  | Database hostname                      |

## Project Structure

```text
SDR-project/
├── k8s/                      # Kubernetes manifests
│   ├── namespace.yaml
│   ├── configmap.yaml
│   ├── secret.yaml
│   ├── rtl-sdr-capture/
│   ├── rust-dsp/
│   ├── timescaledb/
│   └── websocket-gateway/
├── services/
│   ├── rtl-sdr-capture/      # C SDR capture service
│   ├── rust-dsp/             # Rust DSP processor
│   ├── websocket-gateway/    # Rust WebSocket server
│   └── timescaledb/          # Database init scripts
├── frontend/                 # Browser client (JS + Leaflet)
├── proto/                    # Protobuf definitions
├── scripts/                  # Build/deploy scripts
└── README.md
```

## API Endpoints

| Endpoint                   | Method    | Description               |
| -------------------------- | --------- | ------------------------- |
| `/ws`                      | WebSocket | Real-time aircraft updates |
| `/api/aircraft`            | GET       | List all current aircraft |
| `/api/aircraft/:icao/trail` | GET       | Get aircraft trail/history |
| `/health`                  | GET       | Health check              |

## ADS-B Data Fields

Each aircraft message includes:

- **ICAO Address**: Unique 24-bit aircraft identifier
- **Callsign**: Flight number (e.g., "KAL123")
- **Position**: Latitude/Longitude
- **Altitude**: Barometric altitude in feet
- **Ground Speed**: Speed in knots
- **Heading**: Direction in degrees
- **Vertical Rate**: Climb/descent rate in ft/min
- **Squawk**: Transponder code

## Troubleshooting

### RTL-SDR Not Detected

1. Install Zadig driver (Windows) or librtlsdr (Linux)
2. Check USB connection: `rtl_test`
3. Verify device permissions in container

### No Aircraft Appearing

1. Ensure antenna is connected and positioned well
2. Check signal gain settings
3. Verify you're in range of aircraft (1090 MHz line-of-sight)

### Database Connection Issues

1. Check TimescaleDB is running: `kubectl -n adsb-tracker get pods`
2. Verify credentials in ConfigMap/Secret
3. Check network connectivity between pods

### Check Pod Logs

```bash
kubectl -n adsb-tracker logs -f deployment/websocket-gateway
kubectl -n adsb-tracker logs -f deployment/rust-dsp
kubectl -n adsb-tracker logs -f statefulset/timescaledb
```

## License

MIT License

## Acknowledgments

- RTL-SDR community for hardware documentation
- dump1090 project for ADS-B decoding reference
- Leaflet.js for mapping library
