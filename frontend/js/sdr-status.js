/**
 * SDR Status and Signal Visualization Module
 * Displays continuous real-time signal strength at 1090 MHz
 */

const SDRStatus = (function() {
    // DOM elements
    let sdrPanel;
    let toggleSdrBtn;
    let sdrStatusIndicator;
    let sdrStatusText;
    // Device section
    let deviceStatus;
    let deviceSerial;
    let frequency;
    let sampleRate;
    let gain;
    // Signal section
    let sdrSignalDbfs;
    let sdrNoiseDbfs;
    let sdrSnrDb;
    let signalBar;
    let noiseBar;
    let signalChart;
    let chartCtx;
    // Real-time meter
    let signalMeter;
    let meterCtx;
    let signalUpdateIndicator;
    let signalRateDisplay;
    // Decoder section
    let msgRate;
    let sdrPreambles;
    let sdrFrames;
    let sdrCrcErrors;
    let sdrCorrected;
    let sdrSuccessRate;

    // Signal history for chart (per device)
    const signalHistory = {};  // device_id -> array of data points
    const maxHistoryLength = 60; // 60 seconds of data

    // Real-time meter history (short buffer for waterfall effect)
    const meterHistory = [];
    const maxMeterHistory = 60;  // 60 data points for waterfall

    // Current device (for multi-device support)
    let currentDeviceId = null;

    // Update interval for status polling (fallback)
    let updateInterval;

    // Animation frame for smooth meter updates
    let animationFrame = null;
    let targetSignal = -60;
    let currentSignal = -60;
    let targetNoise = -60;
    let currentNoise = -60;

    // Signal update rate tracking
    let signalUpdateCount = 0;
    let lastRateUpdate = Date.now();

    // Last signal update time (for detecting stale data)
    let lastSignalTime = 0;

    function init() {
        // Get DOM elements
        sdrPanel = document.getElementById('sdr-panel');
        toggleSdrBtn = document.getElementById('toggle-sdr-panel');
        sdrStatusIndicator = document.getElementById('sdr-status-indicator');
        sdrStatusText = document.getElementById('sdr-status-text');
        // Device section
        deviceStatus = document.getElementById('sdr-device-status');
        deviceSerial = document.getElementById('sdr-device-serial');
        frequency = document.getElementById('sdr-frequency');
        sampleRate = document.getElementById('sdr-sample-rate');
        gain = document.getElementById('sdr-gain');
        // Signal section
        sdrSignalDbfs = document.getElementById('sdr-signal-dbfs');
        sdrNoiseDbfs = document.getElementById('sdr-noise-dbfs');
        sdrSnrDb = document.getElementById('sdr-snr-db');
        signalBar = document.getElementById('signal-bar');
        noiseBar = document.getElementById('noise-bar');
        signalChart = document.getElementById('signal-chart');
        // Real-time meter
        signalMeter = document.getElementById('signal-meter');
        signalUpdateIndicator = document.getElementById('signal-update-indicator');
        signalRateDisplay = document.getElementById('signal-rate');
        // Decoder section
        msgRate = document.getElementById('sdr-msg-rate');
        sdrPreambles = document.getElementById('sdr-preambles');
        sdrFrames = document.getElementById('sdr-frames');
        sdrCrcErrors = document.getElementById('sdr-crc-errors');
        sdrCorrected = document.getElementById('sdr-corrected');
        sdrSuccessRate = document.getElementById('sdr-success-rate');

        if (signalChart) {
            chartCtx = signalChart.getContext('2d');
        }

        if (signalMeter) {
            meterCtx = signalMeter.getContext('2d');
            // Initialize meter display
            drawMeter();
        }

        // Setup event listeners
        if (toggleSdrBtn) {
            toggleSdrBtn.addEventListener('click', function() {
                sdrPanel.classList.toggle('collapsed');
            });
        }

        // Start animation loop for smooth meter updates
        startAnimationLoop();

        // Start periodic status updates (fallback for when no live signal data)
        fetchSDRStatus();
        updateInterval = setInterval(fetchSDRStatus, 5000);

        // Update rate display periodically
        setInterval(updateSignalRate, 1000);

        console.log('SDR Status module initialized with real-time signal display');
    }

    /**
     * Animation loop for smooth signal meter updates
     */
    function startAnimationLoop() {
        function animate() {
            // Smooth interpolation towards target values
            const smoothing = 0.3;
            currentSignal += (targetSignal - currentSignal) * smoothing;
            currentNoise += (targetNoise - currentNoise) * smoothing;

            // Redraw meter with smooth values
            drawMeter();

            animationFrame = requestAnimationFrame(animate);
        }
        animate();
    }

    /**
     * Update signal rate display
     */
    function updateSignalRate() {
        const now = Date.now();
        const elapsed = (now - lastRateUpdate) / 1000;
        const rate = signalUpdateCount / elapsed;

        if (signalRateDisplay) {
            signalRateDisplay.textContent = rate.toFixed(1) + ' Hz';
        }

        signalUpdateCount = 0;
        lastRateUpdate = now;
    }

    /**
     * Handle device status update from WebSocket
     */
    function handleDeviceStatus(data) {
        const connected = data.connected || false;
        const devId = data.device_id || 'unknown';

        console.log('Device status update:', data);

        // Update current device
        currentDeviceId = devId;

        // Parse device ID to extract serial number
        let serial = devId;
        if (devId.startsWith('RTL-SDR-')) {
            serial = devId.substring(8);
        }

        // Update header indicators
        if (sdrStatusIndicator) {
            sdrStatusIndicator.className = 'status-indicator ' + (connected ? 'connected' : 'disconnected');
        }

        if (sdrStatusText) {
            sdrStatusText.textContent = 'SDR: ' + (connected ? 'Connected' : 'Disconnected');
        }

        // Update panel content
        if (deviceStatus) {
            deviceStatus.textContent = connected ? 'Connected' : 'Disconnected';
            deviceStatus.className = 'sdr-value status-' + (connected ? 'active' : 'disconnected');
        }

        if (deviceSerial) {
            deviceSerial.textContent = serial;
        }

        if (frequency && data.center_freq) {
            const freqMhz = data.center_freq / 1000000;
            frequency.textContent = freqMhz.toFixed(3) + ' MHz';
        }

        if (sampleRate && data.sample_rate) {
            const rateMsps = data.sample_rate / 1000000;
            sampleRate.textContent = rateMsps.toFixed(2) + ' MSPS';
        }

        if (gain && data.gain_db !== undefined) {
            gain.textContent = data.gain_db.toFixed(1) + ' dB';
        }
    }

    /**
     * Handle live signal update from WebSocket
     */
    function handleSignalUpdate(data) {
        const devId = data.device_id || 'unknown';
        lastSignalTime = Date.now();
        signalUpdateCount++;

        // Flash update indicator
        if (signalUpdateIndicator) {
            signalUpdateIndicator.classList.add('active');
            setTimeout(() => signalUpdateIndicator.classList.remove('active'), 100);
        }

        // Set current device if not set
        if (!currentDeviceId) {
            currentDeviceId = devId;
        }

        // Initialize history for this device if needed
        if (!signalHistory[devId]) {
            signalHistory[devId] = [];
        }

        // Add to history
        signalHistory[devId].push({
            time: data.timestamp_ms || Date.now(),
            signal: data.signal_dbfs,
            noise: data.noise_dbfs,
            snr: data.snr_db,
            msgRate: data.msg_rate
        });

        // Keep history limited
        while (signalHistory[devId].length > maxHistoryLength) {
            signalHistory[devId].shift();
        }

        // Add to meter history (waterfall)
        meterHistory.push({
            signal: data.signal_dbfs || -60,
            noise: data.noise_dbfs || -60,
            time: Date.now()
        });
        while (meterHistory.length > maxMeterHistory) {
            meterHistory.shift();
        }

        // Update target values for smooth animation
        targetSignal = data.signal_dbfs || -60;
        targetNoise = data.noise_dbfs || -60;

        // Update display if this is the current device
        if (devId === currentDeviceId) {
            updateLiveSignalDisplay(data);
            drawSignalChart();
        }

        // Update status indicator to show live data (use "Connected" for active state)
        if (sdrStatusIndicator) {
            sdrStatusIndicator.className = 'status-indicator connected';
        }
        if (sdrStatusText) {
            sdrStatusText.textContent = 'SDR: Connected';
        }
        if (deviceStatus) {
            deviceStatus.textContent = 'Connected';
            deviceStatus.className = 'sdr-value status-connected';
        }
    }

    /**
     * Update display with live signal data
     */
    function updateLiveSignalDisplay(data) {
        // Update message rate
        if (msgRate) {
            msgRate.textContent = (data.msg_rate || 0).toFixed(1);
        }

        // Update signal section
        if (sdrSignalDbfs) {
            const signal = data.signal_dbfs;
            sdrSignalDbfs.textContent = (signal !== null && signal !== undefined)
                ? signal.toFixed(1) + ' dBFS' : '- dBFS';
        }

        if (sdrNoiseDbfs) {
            const noise = data.noise_dbfs;
            sdrNoiseDbfs.textContent = (noise !== null && noise !== undefined)
                ? noise.toFixed(1) + ' dBFS' : '- dBFS';
        }

        if (sdrSnrDb) {
            const snr = data.snr_db;
            sdrSnrDb.textContent = (snr !== null && snr !== undefined)
                ? snr.toFixed(1) + ' dB' : '- dB';
        }

        // Update decoder statistics
        if (sdrPreambles) {
            sdrPreambles.textContent = formatNumber(data.preambles_detected || 0);
        }

        if (sdrFrames) {
            sdrFrames.textContent = formatNumber(data.frames_decoded || 0);
        }

        if (sdrCrcErrors) {
            sdrCrcErrors.textContent = formatNumber(data.crc_errors || 0);
        }

        if (sdrCorrected) {
            sdrCorrected.textContent = formatNumber(data.corrected_frames || 0);
        }

        // Calculate and display success rate
        if (sdrSuccessRate) {
            const preambles = data.preambles_detected || 0;
            const frames = data.frames_decoded || 0;
            if (preambles > 0) {
                const rate = (frames / preambles * 100).toFixed(1);
                sdrSuccessRate.textContent = rate + '%';
            } else {
                sdrSuccessRate.textContent = '-%';
            }
        }

        // Update signal bars
        updateSignalBars(data.signal_dbfs, data.noise_dbfs);
    }

    /**
     * Format large numbers with K/M suffixes
     */
    function formatNumber(num) {
        if (num >= 1000000) {
            return (num / 1000000).toFixed(1) + 'M';
        } else if (num >= 1000) {
            return (num / 1000).toFixed(1) + 'K';
        }
        return num.toString();
    }

    async function fetchSDRStatus() {
        try {
            const response = await fetch('/api/sdr/status');
            if (!response.ok) {
                throw new Error('Failed to fetch SDR status');
            }
            const data = await response.json();
            updateSDRDisplay(data);
        } catch (err) {
            console.warn('SDR status fetch error:', err);
            updateSDRDisplay({
                connected: false,
                status: 'disconnected',
                error: 'Unable to fetch status'
            });
        }
    }

    function updateSDRDisplay(data) {
        const status = data.status || 'disconnected';

        // Check if we have recent live signal data (within last 5 seconds)
        const hasLiveSignal = (Date.now() - lastSignalTime) < 5000;

        // Update header indicators (prefer live signal status)
        // Use "Connected" consistently instead of "Active"
        if (!hasLiveSignal) {
            if (sdrStatusIndicator) {
                sdrStatusIndicator.className = 'status-indicator ' +
                    (status === 'active' ? 'connected' :
                     status === 'stale' ? 'connecting' : 'disconnected');
            }

            if (sdrStatusText) {
                sdrStatusText.textContent = 'SDR: ' +
                    (status === 'active' ? 'Connected' :
                     status === 'stale' ? 'Stale' : 'Disconnected');
            }

            // Update panel content
            if (deviceStatus) {
                const displayStatus = status === 'active' ? 'Connected' :
                    status.charAt(0).toUpperCase() + status.slice(1);
                deviceStatus.textContent = displayStatus;
                deviceStatus.className = 'sdr-value status-' + (status === 'active' ? 'connected' : status);
            }
        }

        if (frequency && data.center_freq) {
            frequency.textContent = (data.center_freq / 1000000).toFixed(3) + ' MHz';
        }

        if (sampleRate && data.sample_rate) {
            sampleRate.textContent = (data.sample_rate / 1000000).toFixed(2) + ' MSPS';
        }

        if (gain && data.gain_db !== null) {
            gain.textContent = data.gain_db.toFixed(1) + ' dB';
        }

        // Only update msg rate from API if no live signal
        if (msgRate && !hasLiveSignal) {
            msgRate.textContent = (data.messages_per_second || 0).toFixed(1);
        }

        // Update signal bars from API if no live signal
        if (!hasLiveSignal) {
            updateSignalBars(data.signal_power_db, data.noise_floor_db);
        }

        // Draw chart
        drawSignalChart();
    }

    function updateSignalBars(signalDb, noiseDb) {
        const minDb = -60;
        const maxDb = 0;

        if (signalDb !== null && signalDb !== undefined && signalBar) {
            const signalPercent = Math.max(0, Math.min(100,
                ((signalDb - minDb) / (maxDb - minDb)) * 100));
            signalBar.style.width = signalPercent + '%';
        }

        if (noiseDb !== null && noiseDb !== undefined && noiseBar) {
            const noisePercent = Math.max(0, Math.min(100,
                ((noiseDb - minDb) / (maxDb - minDb)) * 100));
            noiseBar.style.width = noisePercent + '%';
        }
    }

    /**
     * Draw real-time signal meter with waterfall effect
     */
    function drawMeter() {
        if (!meterCtx) return;

        const width = signalMeter.width;
        const height = signalMeter.height;
        const minDb = -60;
        const maxDb = 0;

        // Clear canvas
        meterCtx.fillStyle = '#0a0a15';
        meterCtx.fillRect(0, 0, width, height);

        // Draw waterfall history (scrolling left)
        if (meterHistory.length > 0) {
            const barWidth = width / maxMeterHistory;

            meterHistory.forEach((point, i) => {
                const x = i * barWidth;

                // Signal strength as color intensity
                const signalNorm = Math.max(0, Math.min(1, (point.signal - minDb) / (maxDb - minDb)));
                const noiseNorm = Math.max(0, Math.min(1, (point.noise - minDb) / (maxDb - minDb)));

                // Draw signal bar (green intensity based on strength)
                const signalHeight = signalNorm * height;
                const gradient = meterCtx.createLinearGradient(x, height - signalHeight, x, height);
                gradient.addColorStop(0, `rgba(74, 222, 128, ${0.3 + signalNorm * 0.7})`);
                gradient.addColorStop(1, `rgba(74, 222, 128, 0.1)`);
                meterCtx.fillStyle = gradient;
                meterCtx.fillRect(x, height - signalHeight, barWidth - 1, signalHeight);

                // Draw noise floor line
                const noiseY = height - (noiseNorm * height);
                meterCtx.fillStyle = 'rgba(248, 113, 113, 0.6)';
                meterCtx.fillRect(x, noiseY - 1, barWidth - 1, 2);
            });
        }

        // Draw current signal indicator on the right
        const currentNorm = Math.max(0, Math.min(1, (currentSignal - minDb) / (maxDb - minDb)));
        const indicatorX = width - 10;
        const indicatorHeight = currentNorm * height;

        // Glow effect for current signal
        const glowGradient = meterCtx.createLinearGradient(indicatorX - 5, 0, indicatorX + 10, 0);
        glowGradient.addColorStop(0, 'rgba(74, 222, 128, 0)');
        glowGradient.addColorStop(0.5, `rgba(74, 222, 128, ${currentNorm})`);
        glowGradient.addColorStop(1, 'rgba(74, 222, 128, 0)');
        meterCtx.fillStyle = glowGradient;
        meterCtx.fillRect(indicatorX - 5, height - indicatorHeight, 15, indicatorHeight);

        // Draw scale marks
        meterCtx.strokeStyle = 'rgba(255, 255, 255, 0.2)';
        meterCtx.lineWidth = 1;
        for (let db = -50; db <= 0; db += 10) {
            const y = height - ((db - minDb) / (maxDb - minDb)) * height;
            meterCtx.beginPath();
            meterCtx.moveTo(0, y);
            meterCtx.lineTo(5, y);
            meterCtx.stroke();
        }
    }

    function drawSignalChart() {
        if (!chartCtx) return;

        // Get history for current device
        const history = currentDeviceId ? signalHistory[currentDeviceId] : [];
        if (!history || history.length < 2) return;

        const width = signalChart.width;
        const height = signalChart.height;
        const padding = 5;

        // Clear canvas
        chartCtx.fillStyle = '#1a1a2e';
        chartCtx.fillRect(0, 0, width, height);

        // Draw grid
        chartCtx.strokeStyle = '#0f3460';
        chartCtx.lineWidth = 1;

        // Horizontal lines (signal levels) with labels
        const dbLevels = [-60, -45, -30, -15, 0];
        chartCtx.fillStyle = '#444';
        chartCtx.font = '9px monospace';
        for (let i = 0; i < dbLevels.length; i++) {
            const y = dbToY(dbLevels[i], height, padding);
            chartCtx.beginPath();
            chartCtx.moveTo(padding, y);
            chartCtx.lineTo(width - padding, y);
            chartCtx.stroke();

            // Label
            chartCtx.fillText(dbLevels[i] + '', 2, y - 2);
        }

        // Draw SNR area (shaded region between signal and noise)
        if (history.length > 0) {
            chartCtx.fillStyle = 'rgba(74, 222, 128, 0.15)';
            chartCtx.beginPath();

            history.forEach((point, i) => {
                const x = padding + (i / (maxHistoryLength - 1)) * (width - 2 * padding);
                const signalY = dbToY(point.signal, height, padding);

                if (i === 0) {
                    chartCtx.moveTo(x, signalY);
                } else {
                    chartCtx.lineTo(x, signalY);
                }
            });

            // Close the path along noise floor
            for (let i = history.length - 1; i >= 0; i--) {
                const point = history[i];
                const x = padding + (i / (maxHistoryLength - 1)) * (width - 2 * padding);
                const noiseY = dbToY(point.noise || -60, height, padding);
                chartCtx.lineTo(x, noiseY);
            }

            chartCtx.closePath();
            chartCtx.fill();
        }

        // Draw noise floor line
        if (history.length > 0) {
            chartCtx.strokeStyle = 'rgba(248, 113, 113, 0.7)';
            chartCtx.lineWidth = 1.5;
            chartCtx.beginPath();

            history.forEach((point, i) => {
                const x = padding + (i / (maxHistoryLength - 1)) * (width - 2 * padding);
                const y = dbToY(point.noise || -60, height, padding);

                if (i === 0) {
                    chartCtx.moveTo(x, y);
                } else {
                    chartCtx.lineTo(x, y);
                }
            });
            chartCtx.stroke();
        }

        // Draw signal line
        if (history.length > 0) {
            chartCtx.strokeStyle = '#4ade80';
            chartCtx.lineWidth = 2;
            chartCtx.beginPath();

            history.forEach((point, i) => {
                const x = padding + (i / (maxHistoryLength - 1)) * (width - 2 * padding);
                const y = dbToY(point.signal, height, padding);

                if (i === 0) {
                    chartCtx.moveTo(x, y);
                } else {
                    chartCtx.lineTo(x, y);
                }
            });
            chartCtx.stroke();

            // Draw dots at each data point
            chartCtx.fillStyle = '#4ade80';
            history.forEach((point, i) => {
                const x = padding + (i / (maxHistoryLength - 1)) * (width - 2 * padding);
                const y = dbToY(point.signal, height, padding);
                chartCtx.beginPath();
                chartCtx.arc(x, y, 2, 0, Math.PI * 2);
                chartCtx.fill();
            });
        }

        // Draw legend
        chartCtx.fillStyle = '#4ade80';
        chartCtx.fillRect(width - 70, 5, 10, 3);
        chartCtx.fillStyle = '#888';
        chartCtx.fillText('Signal', width - 55, 10);

        chartCtx.fillStyle = '#f87171';
        chartCtx.fillRect(width - 70, 15, 10, 3);
        chartCtx.fillStyle = '#888';
        chartCtx.fillText('Noise', width - 55, 20);
    }

    function dbToY(db, height, padding) {
        const minDb = -60;
        const maxDb = 0;
        const clampedDb = Math.max(minDb, Math.min(maxDb, db || minDb));
        const normalized = (clampedDb - minDb) / (maxDb - minDb);
        return height - padding - (normalized * (height - 2 * padding));
    }

    function destroy() {
        if (updateInterval) {
            clearInterval(updateInterval);
        }
        if (animationFrame) {
            cancelAnimationFrame(animationFrame);
        }
    }

    return {
        init,
        destroy,
        fetchStatus: fetchSDRStatus,
        handleSignalUpdate,
        handleDeviceStatus,
    };
})();

// Initialize when DOM is ready
document.addEventListener('DOMContentLoaded', function() {
    SDRStatus.init();
});
