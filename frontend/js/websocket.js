/**
 * WebSocket module - handles real-time connection to server
 */

const WebSocketClient = (function() {
    let socket = null;
    let reconnectAttempts = 0;
    let maxReconnectAttempts = 10;
    let reconnectDelay = 1000;
    let onMessageCallback = null;
    let onStatusChangeCallback = null;

    // Get WebSocket URL
    function getWsUrl() {
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const host = window.location.host;
        return `${protocol}//${host}/ws`;
    }

    // Update connection status
    function setStatus(status) {
        if (onStatusChangeCallback) {
            onStatusChangeCallback(status);
        }
    }

    // Connect to WebSocket server
    function connect() {
        if (socket && (socket.readyState === WebSocket.CONNECTING || socket.readyState === WebSocket.OPEN)) {
            return;
        }

        setStatus('connecting');

        const url = getWsUrl();
        console.log('Connecting to WebSocket:', url);

        try {
            socket = new WebSocket(url);
        } catch (e) {
            console.error('Failed to create WebSocket:', e);
            setStatus('disconnected');
            scheduleReconnect();
            return;
        }

        socket.onopen = function() {
            console.log('WebSocket connected');
            setStatus('connected');
            reconnectAttempts = 0;

            // Subscribe to all updates
            send({
                type: 'subscribe',
                icao_filter: []
            });
        };

        socket.onmessage = function(event) {
            try {
                const data = JSON.parse(event.data);
                if (onMessageCallback) {
                    onMessageCallback(data);
                }
            } catch (e) {
                console.error('Failed to parse message:', e);
            }
        };

        socket.onerror = function(error) {
            console.error('WebSocket error:', error);
        };

        socket.onclose = function(event) {
            console.log('WebSocket closed:', event.code, event.reason);
            setStatus('disconnected');
            socket = null;
            scheduleReconnect();
        };
    }

    // Schedule reconnection
    function scheduleReconnect() {
        if (reconnectAttempts >= maxReconnectAttempts) {
            console.log('Max reconnect attempts reached');
            return;
        }

        reconnectAttempts++;
        const delay = Math.min(reconnectDelay * Math.pow(2, reconnectAttempts - 1), 30000);

        console.log(`Reconnecting in ${delay}ms (attempt ${reconnectAttempts}/${maxReconnectAttempts})`);

        setTimeout(connect, delay);
    }

    // Send message to server
    function send(message) {
        if (socket && socket.readyState === WebSocket.OPEN) {
            socket.send(JSON.stringify(message));
            return true;
        }
        return false;
    }

    // Disconnect
    function disconnect() {
        if (socket) {
            socket.close();
            socket = null;
        }
    }

    // Set message callback
    function onMessage(callback) {
        onMessageCallback = callback;
    }

    // Set status change callback
    function onStatusChange(callback) {
        onStatusChangeCallback = callback;
    }

    // Check if connected
    function isConnected() {
        return socket && socket.readyState === WebSocket.OPEN;
    }

    return {
        connect,
        disconnect,
        send,
        onMessage,
        onStatusChange,
        isConnected,
    };
})();
