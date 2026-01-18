/**
 * Main application module
 */

(function() {
    // DOM elements
    const statusIndicator = document.getElementById('connection-status');
    const statusText = document.getElementById('status-text');
    const aircraftCount = document.getElementById('aircraft-count');
    const aircraftList = document.getElementById('aircraft-list');
    const infoPanel = document.getElementById('info-panel');
    const closePanel = document.getElementById('close-panel');
    const showTrailBtn = document.getElementById('show-trail');
    const followBtn = document.getElementById('follow-aircraft');
    const toggleListBtn = document.getElementById('toggle-list');
    const listPanel = document.getElementById('list-panel');

    // Initialize map
    FlightMap.init('map');

    // Update connection status UI
    function updateStatus(status) {
        statusIndicator.className = 'status-indicator ' + status;

        switch (status) {
            case 'connected':
                statusText.textContent = 'Connected';
                break;
            case 'connecting':
                statusText.textContent = 'Connecting...';
                break;
            case 'disconnected':
                statusText.textContent = 'Disconnected';
                break;
        }
    }

    // Update aircraft count (grouped by device if multiple devices)
    function updateAircraftCount() {
        const total = FlightMap.getAircraftCount();
        const positioned = FlightMap.getPositionedAircraftCount();
        const byDevice = FlightMap.getAircraftCountByDevice();
        const deviceCount = Object.keys(byDevice).length;

        let countText;
        if (deviceCount > 1) {
            // Multiple devices - show breakdown
            const deviceInfo = Object.entries(byDevice)
                .map(([devId, count]) => `${devId}: ${count}`)
                .join(' | ');
            countText = `${total} aircraft (${deviceInfo})`;
        } else if (positioned === total) {
            countText = total + ' aircraft';
        } else {
            countText = positioned + '/' + total + ' aircraft';
        }
        aircraftCount.textContent = countText;
    }

    // Update aircraft list table
    function updateAircraftList() {
        const aircraft = FlightMap.getAllAircraft();
        const selected = FlightMap.getSelected();
        const tableBody = document.getElementById('aircraft-table-body');

        if (!tableBody) return;

        // Sort by callsign/icao
        aircraft.sort((a, b) => {
            const nameA = a.callsign || a.icao;
            const nameB = b.callsign || b.icao;
            return nameA.localeCompare(nameB);
        });

        // Clear table
        tableBody.innerHTML = '';

        // Add aircraft rows
        aircraft.forEach(ac => {
            const hasPosition = ac.lat && ac.lon;
            const row = document.createElement('tr');
            row.className = (ac.icao === selected ? 'selected' : '') +
                (!hasPosition ? ' no-position' : '');

            // Format position
            let positionText = '-';
            if (hasPosition) {
                positionText = `${ac.lat.toFixed(4)}, ${ac.lon.toFixed(4)}`;
            }

            // Format altitude
            let altitudeText = '-';
            if (ac.altitude) {
                altitudeText = ac.altitude.toLocaleString() + ' ft';
            }

            // Format vertical rate with arrow indicator
            let vrateText = '-';
            if (ac.vrate !== undefined && ac.vrate !== null && ac.vrate !== 0) {
                const arrow = ac.vrate > 0 ? '↑' : '↓';
                vrateText = `${arrow} ${Math.abs(ac.vrate).toLocaleString()} fpm`;
            }

            // Format speed
            let speedText = '-';
            if (ac.speed) {
                speedText = Math.round(ac.speed) + ' kts';
            }

            // Format heading
            let headingText = '-';
            if (ac.heading !== undefined && ac.heading !== null) {
                headingText = Math.round(ac.heading) + '°';
            }

            row.innerHTML = `
                <td class="icao-cell">${ac.icao}</td>
                <td class="callsign-cell">${ac.callsign || '-'}</td>
                <td class="position-cell">${positionText}</td>
                <td class="altitude-cell">${altitudeText}</td>
                <td class="vrate-cell ${ac.vrate > 0 ? 'climbing' : ac.vrate < 0 ? 'descending' : ''}">${vrateText}</td>
                <td class="speed-cell">${speedText}</td>
                <td class="heading-cell">${headingText}</td>
                <td class="squawk-cell">${ac.squawk || '-'}</td>
                <td class="device-cell">${ac.device_id || '-'}</td>
            `;

            row.addEventListener('click', function() {
                if (hasPosition) {
                    FlightMap.selectAircraft(ac.icao);
                }
                updateAircraftList();
            });

            tableBody.appendChild(row);
        });
    }

    // Handle WebSocket message
    function handleMessage(data) {
        switch (data.type) {
            case 'initial':
                // Initial aircraft list
                if (data.aircraft && Array.isArray(data.aircraft)) {
                    data.aircraft.forEach(ac => {
                        FlightMap.updateAircraft(ac);
                    });
                    updateAircraftCount();
                    updateAircraftList();
                }
                break;

            case 'update':
                // Single aircraft update (legacy format)
                if (data.aircraft) {
                    FlightMap.updateAircraft(data.aircraft);
                    updateAircraftCount();
                    throttledListUpdate();
                }
                break;

            case 'position_update':
                // Real-time position update from LISTEN/NOTIFY
                FlightMap.updateAircraft({
                    icao: data.icao,
                    device_id: data.device_id,
                    lat: data.lat,
                    lon: data.lon,
                    altitude: data.altitude,
                    speed: data.speed,
                    heading: data.heading,
                    vrate: data.vrate,
                    seen: data.time,
                });
                updateAircraftCount();
                throttledListUpdate();
                break;

            case 'signal':
                // Signal metrics from gRPC (ephemeral - not stored in DB)
                // Forward to SDR status module
                if (typeof SDRStatus !== 'undefined' && SDRStatus.handleSignalUpdate) {
                    SDRStatus.handleSignalUpdate(data);
                }
                break;

            case 'device_status':
                // Device status update from gRPC
                // Forward to SDR status module
                if (typeof SDRStatus !== 'undefined' && SDRStatus.handleDeviceStatus) {
                    SDRStatus.handleDeviceStatus(data);
                }
                break;

            case 'remove':
            case 'aircraft_removed':
                // Aircraft removed (timed out)
                if (data.icao) {
                    FlightMap.removeAircraft(data.icao);
                    updateAircraftCount();
                    updateAircraftList();
                }
                break;
        }
    }

    // Throttle list updates to prevent excessive DOM updates
    function throttledListUpdate() {
        if (!handleMessage.listUpdatePending) {
            handleMessage.listUpdatePending = true;
            setTimeout(() => {
                updateAircraftList();
                handleMessage.listUpdatePending = false;
            }, 1000);
        }
    }

    // Setup WebSocket
    WebSocketClient.onStatusChange(updateStatus);
    WebSocketClient.onMessage(handleMessage);
    WebSocketClient.connect();

    // Event listeners
    closePanel.addEventListener('click', function() {
        FlightMap.deselectAircraft();
        updateAircraftList();
    });

    showTrailBtn.addEventListener('click', function() {
        const selected = FlightMap.getSelected();
        if (selected) {
            FlightMap.showTrail(selected);
        }
    });

    followBtn.addEventListener('click', function() {
        const selected = FlightMap.getSelected();
        if (selected) {
            const following = FlightMap.toggleFollow(selected);
            followBtn.textContent = following ? 'Unfollow' : 'Follow';
        }
    });

    toggleListBtn.addEventListener('click', function() {
        listPanel.classList.toggle('collapsed');
    });

    // Periodic cleanup of stale aircraft
    setInterval(function() {
        const removed = FlightMap.cleanupStale();
        if (removed > 0) {
            console.log(`Removed ${removed} stale aircraft`);
            updateAircraftCount();
            updateAircraftList();
        }
    }, 30000);

    // Fetch initial data via REST API as backup
    fetch('/api/aircraft')
        .then(response => response.json())
        .then(aircraft => {
            aircraft.forEach(ac => {
                FlightMap.updateAircraft(ac);
            });
            updateAircraftCount();
            updateAircraftList();
        })
        .catch(err => {
            console.log('Initial fetch failed (will use WebSocket):', err);
        });

    console.log('ADS-B Flight Tracker initialized');
})();
