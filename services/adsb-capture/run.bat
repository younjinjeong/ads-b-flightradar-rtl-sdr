@echo off
REM SDR ADS-B Capture - Run Script
REM Uses rtl_adsb.exe from lib folder to capture ADS-B data

set "GATEWAY_URL=http://localhost:30051"
set "DEVICE_INDEX=0"
set "DEVICE_ID=rtlsdr-0"
set "DEVICE_GAIN=49.6"
set "PPM_ERROR=0"
set "RTL_ADSB_PATH=%~dp0lib\rtl_adsb.exe"

REM Ensure DLLs are in PATH (same directory as rtl_adsb.exe)
set "PATH=%~dp0lib;%PATH%"

"%~dp0target\release\adsb-capture.exe"
