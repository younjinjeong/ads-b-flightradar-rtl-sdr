/**
 * Map module - handles Leaflet map and aircraft markers
 */

const FlightMap = (function() {
    let map;
    let aircraftMarkers = {};  // Aircraft with position (on map)
    let aircraftData = {};     // All aircraft data (including those without position)
    let aircraftTrails = {};   // Manual trails fetched from API
    let aircraftRecentPositions = {};  // Store last 5 positions per aircraft for auto-trail
    let selectedAircraft = null;
    let followingAircraft = null;
    const MAX_TRAIL_POINTS = 5;  // Number of trail points to show

    // Aircraft SVG icon
    const aircraftSvg = `
        <svg viewBox="0 0 24 24" width="32" height="32">
            <path fill="currentColor" d="M21,16V14L13,9V3.5A1.5,1.5,0,0,0,11.5,2A1.5,1.5,0,0,0,10,3.5V9L2,14V16L10,13.5V19L8,20.5V22L11.5,21L15,22V20.5L13,19V13.5L21,16Z"/>
        </svg>
    `;

    // Color based on altitude
    function getAltitudeColor(altitude) {
        if (!altitude || altitude <= 0) return '#888888';
        if (altitude < 10000) return '#4ade80';  // Green - low
        if (altitude < 20000) return '#fbbf24';  // Yellow - medium
        if (altitude < 30000) return '#f97316';  // Orange - high
        return '#ef4444';  // Red - very high
    }

    // Create aircraft icon
    function createAircraftIcon(heading, altitude) {
        const color = getAltitudeColor(altitude);
        const rotation = heading || 0;

        return L.divIcon({
            className: 'aircraft-marker',
            html: `<div class="aircraft-icon" style="transform: rotate(${rotation}deg); color: ${color};">
                ${aircraftSvg}
            </div>`,
            iconSize: [32, 32],
            iconAnchor: [16, 16],
        });
    }

    // Initialize map
    function init(containerId, center, zoom) {
        // Create map
        map = L.map(containerId, {
            center: center || [37.5665, 126.9780], // Seoul default
            zoom: zoom || 8,
            zoomControl: true,
            attributionControl: true,
        });

        // Add tile layer (dark theme)
        L.tileLayer('https://{s}.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}{r}.png', {
            attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> &copy; <a href="https://carto.com/attributions">CARTO</a>',
            subdomains: 'abcd',
            maxZoom: 19
        }).addTo(map);

        // Map click handler - deselect aircraft
        map.on('click', function() {
            if (selectedAircraft) {
                deselectAircraft();
            }
        });

        return map;
    }

    // Update or create aircraft marker
    function updateAircraft(aircraft) {
        const icao = aircraft.icao;

        // Always store aircraft data (even without position)
        aircraftData[icao] = {
            ...aircraftData[icao],
            ...aircraft,
            lastUpdate: Date.now()
        };

        if (!aircraft.lat || !aircraft.lon) {
            return; // No position data for map marker
        }

        const position = [aircraft.lat, aircraft.lon];
        const icon = createAircraftIcon(aircraft.heading, aircraft.altitude);

        // Track recent positions for auto-trail (last 5 unique positions)
        if (!aircraftRecentPositions[icao]) {
            aircraftRecentPositions[icao] = [];
        }
        const recentPos = aircraftRecentPositions[icao];
        const lastPos = recentPos.length > 0 ? recentPos[recentPos.length - 1] : null;

        // Only add if position changed significantly (more than ~100m)
        if (!lastPos ||
            Math.abs(lastPos[0] - position[0]) > 0.001 ||
            Math.abs(lastPos[1] - position[1]) > 0.001) {
            recentPos.push([...position]);
            if (recentPos.length > MAX_TRAIL_POINTS) {
                recentPos.shift();
            }
        }

        if (aircraftMarkers[icao]) {
            // Update existing marker
            const marker = aircraftMarkers[icao];
            marker.setLatLng(position);
            marker.setIcon(icon);
            marker.aircraft = aircraft;

            // Update auto-trail (always visible, last 5 positions)
            updateAutoTrail(icao);

            // Update manual trail if exists (from "Show Trail" button)
            if (aircraftTrails[icao]) {
                const trail = aircraftTrails[icao];
                const latlngs = trail.getLatLngs();
                latlngs.push(position);

                // Keep only last 100 points
                if (latlngs.length > 100) {
                    latlngs.shift();
                }
                trail.setLatLngs(latlngs);
            }
        } else {
            // Create new marker
            const marker = L.marker(position, {
                icon: icon,
                title: aircraft.callsign || aircraft.icao,
            }).addTo(map);

            marker.aircraft = aircraft;

            // Click handler
            marker.on('click', function(e) {
                L.DomEvent.stopPropagation(e);
                selectAircraft(icao);
            });

            // Tooltip
            marker.bindTooltip(function() {
                const a = marker.aircraft;
                return `<div class="aircraft-label">
                    <strong>${a.callsign || a.icao}</strong><br>
                    ${a.altitude ? a.altitude.toLocaleString() + ' ft' : '-'}
                </div>`;
            }, {
                permanent: false,
                direction: 'top',
                offset: [0, -16],
            });

            aircraftMarkers[icao] = marker;

            // Create initial auto-trail
            updateAutoTrail(icao);
        }

        // Follow aircraft if enabled
        if (followingAircraft === icao) {
            map.panTo(position, { animate: true, duration: 0.5 });
        }

        // Update info panel if selected
        if (selectedAircraft === icao) {
            updateInfoPanel(aircraft);
        }
    }

    // Update auto-trail (always visible, shows last 5 positions)
    function updateAutoTrail(icao) {
        const recentPos = aircraftRecentPositions[icao];
        if (!recentPos || recentPos.length < 2) return;

        // Create or get auto-trail layer
        if (!aircraftMarkers[icao].autoTrail) {
            const color = getAltitudeColor(aircraftData[icao]?.altitude);
            aircraftMarkers[icao].autoTrail = L.polyline(recentPos, {
                color: color,
                weight: 2,
                opacity: 0.6,
                dashArray: '5, 5',
            }).addTo(map);
        } else {
            // Update existing trail
            const trail = aircraftMarkers[icao].autoTrail;
            trail.setLatLngs(recentPos);
            // Update color based on altitude
            const color = getAltitudeColor(aircraftData[icao]?.altitude);
            trail.setStyle({ color: color });
        }
    }

    // Remove stale aircraft
    function removeAircraft(icao) {
        if (aircraftMarkers[icao]) {
            // Remove auto-trail if exists
            if (aircraftMarkers[icao].autoTrail) {
                map.removeLayer(aircraftMarkers[icao].autoTrail);
            }
            map.removeLayer(aircraftMarkers[icao]);
            delete aircraftMarkers[icao];
        }

        if (aircraftTrails[icao]) {
            map.removeLayer(aircraftTrails[icao]);
            delete aircraftTrails[icao];
        }

        // Remove recent positions
        delete aircraftRecentPositions[icao];

        // Remove from data store
        delete aircraftData[icao];

        if (selectedAircraft === icao) {
            deselectAircraft();
        }

        if (followingAircraft === icao) {
            followingAircraft = null;
        }
    }

    // Select aircraft
    function selectAircraft(icao) {
        selectedAircraft = icao;
        const marker = aircraftMarkers[icao];

        if (marker) {
            updateInfoPanel(marker.aircraft);
            showInfoPanel();
        }
    }

    // Deselect aircraft
    function deselectAircraft() {
        selectedAircraft = null;
        followingAircraft = null;
        hideInfoPanel();
    }

    // Update info panel
    function updateInfoPanel(aircraft) {
        document.getElementById('selected-callsign').textContent =
            aircraft.callsign || aircraft.icao;
        document.getElementById('info-icao').textContent = aircraft.icao;
        document.getElementById('info-callsign').textContent = aircraft.callsign || '-';
        document.getElementById('info-altitude').textContent =
            aircraft.altitude ? aircraft.altitude.toLocaleString() + ' ft' : '-';
        document.getElementById('info-speed').textContent =
            aircraft.speed ? Math.round(aircraft.speed) + ' kts' : '-';
        document.getElementById('info-heading').textContent =
            aircraft.heading ? Math.round(aircraft.heading) + '°' : '-';
        document.getElementById('info-vrate').textContent =
            aircraft.vrate ? aircraft.vrate + ' fpm' : '-';
        document.getElementById('info-squawk').textContent = aircraft.squawk || '-';
        document.getElementById('info-position').textContent =
            aircraft.lat && aircraft.lon ?
                `${aircraft.lat.toFixed(4)}, ${aircraft.lon.toFixed(4)}` : '-';
        document.getElementById('info-seen').textContent =
            aircraft.seen ? new Date(aircraft.seen).toLocaleTimeString() : '-';
        document.getElementById('info-messages').textContent =
            aircraft.messages ? aircraft.messages.toLocaleString() : '-';
    }

    // Show info panel
    function showInfoPanel() {
        document.getElementById('info-panel').classList.remove('hidden');
    }

    // Hide info panel
    function hideInfoPanel() {
        document.getElementById('info-panel').classList.add('hidden');
    }

    // Show aircraft trail
    function showTrail(icao) {
        // Remove existing trail
        if (aircraftTrails[icao]) {
            map.removeLayer(aircraftTrails[icao]);
        }

        // Fetch trail from API
        fetch(`/api/aircraft/${icao}/trail?minutes=30`)
            .then(response => response.json())
            .then(trail => {
                if (trail.length > 0) {
                    const latlngs = trail.map(p => [p.lat, p.lon]);
                    const polyline = L.polyline(latlngs, {
                        color: '#e94560',
                        weight: 2,
                        opacity: 0.7,
                        smoothFactor: 1,
                    }).addTo(map);

                    aircraftTrails[icao] = polyline;
                }
            })
            .catch(err => console.error('Failed to fetch trail:', err));
    }

    // Toggle follow aircraft
    function toggleFollow(icao) {
        if (followingAircraft === icao) {
            followingAircraft = null;
            return false;
        } else {
            followingAircraft = icao;

            // Center on aircraft immediately
            const marker = aircraftMarkers[icao];
            if (marker) {
                map.panTo(marker.getLatLng(), { animate: true });
            }

            return true;
        }
    }

    // Get all aircraft (including those without position)
    function getAllAircraft() {
        return Object.values(aircraftData);
    }

    // Get aircraft count (all detected aircraft)
    function getAircraftCount() {
        return Object.keys(aircraftData).length;
    }

    // Get aircraft with position count (those on map)
    function getPositionedAircraftCount() {
        return Object.keys(aircraftMarkers).length;
    }

    // Get selected aircraft ICAO
    function getSelected() {
        return selectedAircraft;
    }

    // Get aircraft count grouped by device ID
    function getAircraftCountByDevice() {
        const byDevice = {};
        Object.values(aircraftData).forEach(aircraft => {
            const deviceId = aircraft.device_id || 'unknown';
            byDevice[deviceId] = (byDevice[deviceId] || 0) + 1;
        });
        return byDevice;
    }

    // Clean up stale aircraft (not seen in last 5 minutes)
    function cleanupStale(maxAgeMs = 300000) {
        const now = Date.now();
        const stale = [];

        // Check all aircraft data (not just markers)
        Object.keys(aircraftData).forEach(icao => {
            const aircraft = aircraftData[icao];
            const lastUpdate = aircraft.lastUpdate || 0;
            const seen = aircraft.seen ? new Date(aircraft.seen).getTime() : lastUpdate;
            const age = now - Math.max(lastUpdate, seen);
            if (age > maxAgeMs) {
                stale.push(icao);
            }
        });

        stale.forEach(icao => removeAircraft(icao));

        return stale.length;
    }

    return {
        init,
        updateAircraft,
        removeAircraft,
        selectAircraft,
        deselectAircraft,
        showTrail,
        toggleFollow,
        getAllAircraft,
        getAircraftCount,
        getPositionedAircraftCount,
        getAircraftCountByDevice,
        getSelected,
        cleanupStale,
    };
})();
