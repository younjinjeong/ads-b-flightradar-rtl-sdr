# ADS-B Flight Tracker with RTL-SDR

A real-time ADS-B (Automatic Dependent Surveillance-Broadcast) flight tracking system using RTL-SDR hardware, Rust signal processing, and Kubernetes orchestration.

<!-- RTL-SDR Device Image -->
<p align="center">
  <img src="https://upload.wikimedia.org/wikipedia/commons/thumb/d/d8/DVB-T_USB_dongle_with_RTL2832U_and_R820T_%28cropped%29.jpg/440px-DVB-T_USB_dongle_with_RTL2832U_and_R820T_%28cropped%29.jpg" alt="RTL-SDR R820T2 USB Tuner" width="300">
  <br>
  <em>RTL-SDR USB Dongle with RTL2832U + R820T/R820T2 Tuner<br>
  <small>Image: Wikimedia Commons (CC BY-SA 3.0)</small></em>
</p>

## Table of Contents

- [Overview](#overview)
- [How ADS-B Works](#how-ads-b-works)
- [Hardware Requirements](#hardware-requirements)
- [Architecture](#architecture)
- [Quick Start](#quick-start)
- [Signal Processing](#signal-processing)
- [Antenna Guide](#antenna-guide)
- [API Reference](#api-reference)
- [Troubleshooting](#troubleshooting)
- [References](#references)

---

## Overview

This project captures ADS-B signals from aircraft transponders at 1090 MHz using an RTL-SDR USB dongle, decodes the Mode S messages in real-time, and displays aircraft positions on an interactive web map.

### Features

- **Real-time aircraft tracking** via WebSocket streaming
- **Native RTL-SDR capture** using direct IQ sampling at 2 MSPS
- **dump1090-style decoder** implemented in Rust for Mode S/ADS-B
- **Signal visualization** with real-time dBFS meter and history chart
- **Multi-device support** for tracking with multiple SDR receivers
- **TimescaleDB storage** for historical flight data
- **Kubernetes deployment** for containerized services

---

## How ADS-B Works

### What is ADS-B?

**Automatic Dependent Surveillance-Broadcast (ADS-B)** is a surveillance technology where aircraft determine their position via GNSS (GPS) and periodically broadcast it, enabling ground stations and other aircraft to track them.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           ADS-B Signal Flow                                  │
│                                                                              │
│    ✈️ Aircraft                                                               │
│       │                                                                      │
│       │ GPS Position + Altitude + Velocity                                   │
│       │                                                                      │
│       ▼                                                                      │
│    ┌─────────────┐                                                           │
│    │ Transponder │  Mode S Extended Squitter (DF17)                         │
│    │ (1090 MHz)  │  ─────────────────────────────────────►                   │
│    └─────────────┘                                                           │
│                         📡 1090 MHz RF Signal                                │
│                                   │                                          │
│                                   ▼                                          │
│                    ┌──────────────────────────┐                              │
│                    │  RTL-SDR Receiver        │                              │
│                    │  (Ground Station)        │                              │
│                    └──────────────────────────┘                              │
│                                   │                                          │
│                                   ▼                                          │
│                    ┌──────────────────────────┐                              │
│                    │  Decoder (dump1090)      │                              │
│                    │  Position, Callsign,     │                              │
│                    │  Altitude, Speed...      │                              │
│                    └──────────────────────────┘                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

### ADS-B Signal Specifications

| Parameter | Value | Description |
|-----------|-------|-------------|
| **Frequency** | 1090 MHz | Downlink frequency (aircraft to ground) |
| **Wavelength** | 27.5 cm | λ = c / f |
| **Modulation** | PPM (Pulse Position Modulation) | Binary data encoding |
| **Data Rate** | 1 Mbps | 1 microsecond per bit |
| **Message Length** | 112 bits | Extended Squitter (DF17) |
| **Transmission Rate** | ~1 Hz | Position broadcast interval |

### Mode S Message Structure

```
┌────────────────────────────────────────────────────────────────────────────┐
│                    Mode S Extended Squitter (112 bits)                      │
├──────────┬──────────┬──────────────┬─────────────────────────┬─────────────┤
│  DF (5)  │  CA (3)  │  ICAO (24)   │        ME (56)          │   PI (24)   │
│ Downlink │Capability│   Address    │     Message/Data        │   Parity    │
│  Format  │          │              │                         │  (CRC-24)   │
├──────────┼──────────┼──────────────┼─────────────────────────┼─────────────┤
│  10001   │   101    │   4840D6     │    202CC371C32CE0       │   576098    │
│  (DF=17) │  (CA=5)  │  (Aircraft)  │   (Position/Velocity)   │  (Checksum) │
└──────────┴──────────┴──────────────┴─────────────────────────┴─────────────┘

Preamble (8μs):
    ┌─┐   ┌─┐         ┌─┐   ┌─┐
    │ │   │ │         │ │   │ │
────┘ └───┘ └─────────┘ └───┘ └────────────────────────
    0.5 1.0   2.0      3.5 4.0 4.5    (microseconds)
```

### Message Types (Type Code in ME field)

| TC | Type | Description |
|----|------|-------------|
| 1-4 | Aircraft ID | Callsign (flight number) |
| 5-8 | Surface Position | Ground position |
| 9-18 | Airborne Position | Latitude, Longitude, Altitude |
| 19 | Airborne Velocity | Ground speed, Heading, Vertical rate |
| 28 | Emergency Status | Emergency/priority codes |
| 29 | Target State | Autopilot settings |
| 31 | Operational Status | ADS-B version, capabilities |

### CPR Position Decoding

ADS-B uses **Compact Position Reporting (CPR)** to encode latitude/longitude in 17 bits each. Two message types are alternated:

- **Even frame** (F=0): Uses 60 latitude zones
- **Odd frame** (F=1): Uses 59 latitude zones

Global position is recovered by combining both frames received within 10 seconds.

---

## Hardware Requirements

### RTL-SDR Device

This project was developed and tested with:

**RTL-SDR R820T2 USB Tuner**
- Chipset: RTL2832U + R820T2
- Frequency Range: 100 kHz - 1.7 GHz
- Sample Rate: Up to 2.4 MSPS
- ADC Resolution: 8-bit
- Interface: USB 2.0

*See device image at top of page*

### Recommended Setup

| Component | Recommendation |
|-----------|----------------|
| **SDR Receiver** | RTL-SDR Blog V3 or V4 |
| **Antenna** | 1090 MHz tuned antenna (see below) |
| **Cable** | Low-loss coaxial (RG-6 or better) |
| **Location** | Outdoor, elevated, clear sky view |
| **Computer** | Windows 10/11 with USB 2.0+ |

---

## Antenna Guide

### Antenna Specifications for 1090 MHz

The wavelength at 1090 MHz is **27.5 cm** (λ = c/f = 299,792,458 / 1,090,000,000).

| Antenna Type | Length | Gain | Notes |
|--------------|--------|------|-------|
| **Quarter-wave monopole** | 6.875 cm | 2 dBi | Requires ground plane |
| **Half-wave dipole** | 13.75 cm | 2.15 dBi | Simple construction |
| **Collinear** | Variable | 5-8 dBi | Multiple elements |
| **Commercial ADS-B** | - | 3-5 dBi | Tuned for 1090 MHz |

### DIY Quarter-Wave Ground Plane Antenna

```
         ▲ 6.875 cm (vertical element)
         │
         │
    ─────┼─────  Ground plane radials (4x 6.875 cm)
        ╱│╲      at 45° angle downward
       ╱ │ ╲
      ╱  │  ╲
         │
    ═════╪═════  Coax connector (N-type or SMA)
         │
         │ Coaxial cable to SDR
```

### Antenna Placement Tips

1. **Height matters**: Higher placement = longer line-of-sight range
2. **Clear sky view**: Avoid obstructions toward horizon
3. **Away from interference**: Keep away from WiFi routers, computers
4. **Vertical polarization**: ADS-B signals are vertically polarized
5. **Weatherproofing**: Protect connections if outdoor

### Expected Range

| Setup | Typical Range |
|-------|---------------|
| Indoor, stock antenna | 30-80 km |
| Outdoor, stock antenna | 80-150 km |
| Outdoor, tuned antenna | 150-300 km |
| Elevated + LNA | 300-450 km |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                               Windows Host                                   │
│                                                                              │
│   ┌─────────────────┐                                                        │
│   │  RTL-SDR USB    │◄──── 1090 MHz Antenna                                  │
│   │  Device         │                                                        │
│   └────────┬────────┘                                                        │
│            │ USB                                                             │
│            ▼                                                                 │
│   ┌─────────────────┐         ┌─────────────────┐                           │
│   │  rtl_sdr.exe    │────────►│  adsb-capture   │                           │
│   │  (IQ capture)   │  stdout │  (Rust decoder) │                           │
│   └─────────────────┘   pipe  └────────┬────────┘                           │
│                                        │ gRPC                                │
└────────────────────────────────────────│────────────────────────────────────┘
                                         │
                                         ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                          Kubernetes Cluster                                  │
│                                                                              │
│   ┌─────────────────┐         ┌─────────────────┐        ┌────────────────┐ │
│   │  grpc-gateway   │◄───────►│  TimescaleDB    │        │   Browser      │ │
│   │  (Rust/Tonic)   │   SQL   │  (PostgreSQL)   │        │   Client       │ │
│   │                 │         └─────────────────┘        │   (Leaflet)    │ │
│   │  - gRPC server  │                                    │                │ │
│   │  - WebSocket    │◄───────────────────────────────────┤   :30888       │ │
│   │  - REST API     │         WebSocket                  └────────────────┘ │
│   │  - Static files │                                                       │
│   └─────────────────┘                                                       │
│         :30051 (gRPC)                                                       │
│         :30888 (HTTP/WS)                                                    │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Components

| Service | Technology | Description |
|---------|------------|-------------|
| **adsb-capture** | Rust + rtl_sdr | Native Windows app for RTL-SDR IQ capture and Mode S decoding |
| **grpc-gateway** | Rust + Tonic + Axum | gRPC server, WebSocket broadcaster, REST API |
| **TimescaleDB** | PostgreSQL | Time-series database for aircraft positions |
| **Frontend** | Vanilla JS + Leaflet | Interactive map with real-time updates |

---

## Quick Start

### Prerequisites

- Docker Desktop with Kubernetes enabled
- RTL-SDR USB device
- Windows 10/11 (for adsb-capture)

### 1. Deploy Kubernetes Services

```powershell
# Create namespace and deploy services
kubectl apply -f k8s/namespace.yaml
kubectl apply -f k8s/configmap.yaml
kubectl apply -f k8s/secret.yaml
kubectl apply -f k8s/timescaledb/
kubectl apply -f k8s/grpc-gateway/

# Verify pods are running
kubectl get pods -n adsb-tracker
```

### 2. Build and Run adsb-capture

```powershell
cd services/adsb-capture

# Build (requires Rust toolchain)
cargo build --release

# Run
.\run.bat
```

### 3. Access Web UI

Open: **http://localhost:30888**

---

## Signal Processing

### dump1090-Style Decoder

This project implements a Mode S decoder inspired by [dump1090](https://github.com/antirez/dump1090), with the following signal processing pipeline:

```
IQ Samples (2 MSPS)
       │
       ▼
┌──────────────────┐
│ Magnitude        │  mag = √(I² + Q²)
│ Computation      │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Preamble         │  Correlate with known preamble pattern
│ Detection        │  Threshold: signal > 2× noise floor
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Bit Extraction   │  Sample at 2× chip rate
│ (PPM Demod)      │  Compare early vs late samples
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ CRC-24           │  Validate message integrity
│ Validation       │  Attempt 1-bit error correction
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Message          │  Parse DF, ICAO, TC
│ Parsing          │  Decode position, velocity, etc.
└──────────────────┘
```

### Preamble Detection

The Mode S preamble is 8 microseconds:

```
Signal:    ████    ████            ████    ████
           0.5μs   1.0μs           3.5μs   4.0μs

Pattern:   HIGH at 0, 0.5, 3.5, 4.0 μs
           LOW  at 1.0-3.0, 4.5-8.0 μs
```

Detection criteria:
1. Peaks at positions 0, 2, 7, 9 (at 2 MSPS)
2. Valleys at positions 4, 5, 11, 12, 13, 14
3. Signal-to-noise ratio > threshold

### Signal Metrics

The decoder reports real-time signal statistics:

| Metric | Description |
|--------|-------------|
| **Signal dBFS** | Peak signal level relative to full scale |
| **Noise Floor dBFS** | Background noise level |
| **SNR** | Signal-to-noise ratio (dB) |
| **Msg/sec** | Decoded messages per second |
| **Preambles** | Total preambles detected |
| **Frames** | Valid frames decoded |
| **CRC Errors** | Failed CRC validations |
| **Corrected** | 1-bit error corrections |

---

## API Reference

### REST Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/` | GET | Web UI (static files) |
| `/health` | GET | Health check |
| `/api/aircraft` | GET | List all tracked aircraft |
| `/api/aircraft/:icao` | GET | Get specific aircraft |
| `/api/sdr/status` | GET | SDR device status |

### WebSocket Messages

Connect to: `ws://localhost:30888/ws`

**Aircraft Position Update**
```json
{
  "type": "position_update",
  "icao": "4840D6",
  "device_id": "rtlsdr-0",
  "lat": 37.5665,
  "lon": 126.9780,
  "altitude": 35000,
  "speed": 450,
  "heading": 270,
  "vrate": -500,
  "time": "2024-01-15T10:30:00Z"
}
```

**Signal Metrics**
```json
{
  "type": "signal",
  "device_id": "rtlsdr-0",
  "signal_dbfs": -25.5,
  "noise_dbfs": -45.2,
  "snr_db": 19.7,
  "msg_rate": 2.5,
  "preambles_detected": 1500,
  "frames_decoded": 120,
  "crc_errors": 1380
}
```

### ADS-B Data Fields

| Field | Type | Description |
|-------|------|-------------|
| `icao` | String | 24-bit aircraft address (hex) |
| `callsign` | String | Flight number (e.g., "KAL123") |
| `lat` / `lon` | Float | Position in degrees |
| `altitude` | Integer | Barometric altitude (feet) |
| `speed` | Float | Ground speed (knots) |
| `heading` | Float | Track angle (degrees) |
| `vrate` | Integer | Vertical rate (ft/min) |
| `squawk` | String | Transponder code (octal) |

---

## Troubleshooting

### RTL-SDR Not Detected

1. **Install drivers**: Use [Zadig](https://zadig.akeo.ie/) to install WinUSB driver
2. **Test device**: Run `rtl_test -t` to verify
3. **Check USB**: Try different USB port, avoid USB hubs

### No Aircraft Appearing

1. **Check antenna**: Ensure proper 1090 MHz antenna connected
2. **Verify signal**: Check SNR in SDR panel (should be > 10 dB)
3. **Location**: Move antenna to window/outdoor location
4. **Aircraft activity**: Check if aircraft are in range (use FlightRadar24)

### High CRC Error Rate

| Symptom | Likely Cause | Solution |
|---------|--------------|----------|
| >90% CRC errors | Wrong antenna | Use 1090 MHz tuned antenna |
| Intermittent | Weak signal | Add LNA or improve antenna placement |
| All errors | No real signal | Verify aircraft in range |

### Database Connection Issues

```bash
# Check TimescaleDB pod
kubectl -n adsb-tracker get pods
kubectl -n adsb-tracker logs statefulset/timescaledb

# Verify connectivity
kubectl -n adsb-tracker exec -it deployment/grpc-gateway -- nc -zv timescaledb 5432
```

---

## References

### ADS-B and Mode S

- [The 1090 Megahertz Riddle](https://mode-s.org/decode/) - Comprehensive ADS-B decoding book by Junzi Sun
- [Mode-S.org](https://mode-s.org/) - Mode S and ADS-B technical resources
- [dump1090](https://github.com/antirez/dump1090) - Original Mode S decoder by Salvatore Sanfilippo
- [ADS-B - Wikipedia](https://en.wikipedia.org/wiki/Automatic_Dependent_Surveillance–Broadcast)

### RTL-SDR

- [RTL-SDR Blog](https://www.rtl-sdr.com/) - RTL-SDR tutorials and news
- [librtlsdr](https://github.com/steve-m/librtlsdr) - RTL-SDR library
- [Signal Identification Wiki](https://www.sigidwiki.com/wiki/Automatic_Dependent_Surveillance-Broadcast_(ADS-B)) - ADS-B signal reference

### Specifications

- [ICAO Doc 9871](https://www.icao.int/) - Technical provisions for Mode S
- [DO-260B](https://www.rtca.org/) - ADS-B equipment specifications

---

## License

MIT License

---

## Acknowledgments

- [dump1090](https://github.com/antirez/dump1090) by Salvatore Sanfilippo - Mode S decoder reference
- [The 1090MHz Riddle](https://mode-s.org/) by Junzi Sun - ADS-B decoding documentation
- [RTL-SDR community](https://www.rtl-sdr.com/) - Hardware documentation
- [Leaflet.js](https://leafletjs.com/) - Mapping library
- [TimescaleDB](https://www.timescale.com/) - Time-series database
