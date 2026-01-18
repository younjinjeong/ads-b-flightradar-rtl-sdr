#!/bin/bash
set -e

echo "=========================================="
echo "   Deploying ADS-B Tracker to Kubernetes"
echo "=========================================="

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR/k8s"

# Check kubectl is available
if ! command -v kubectl &> /dev/null; then
    echo "Error: kubectl not found. Please install kubectl first."
    exit 1
fi

# Check if kubernetes context is set
echo "Current Kubernetes context:"
kubectl config current-context

read -p "Continue with deployment? (y/n) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    exit 1
fi

# Create namespace
echo ""
echo "[1/6] Creating namespace..."
kubectl apply -f namespace.yaml

# Create ConfigMap and Secret
echo ""
echo "[2/6] Creating ConfigMap and Secret..."
kubectl apply -f configmap.yaml
kubectl apply -f secret.yaml

# Deploy TimescaleDB
echo ""
echo "[3/6] Deploying TimescaleDB..."
kubectl apply -f timescaledb/init-configmap.yaml
kubectl apply -f timescaledb/statefulset.yaml
kubectl apply -f timescaledb/service.yaml

# Wait for TimescaleDB to be ready
echo "Waiting for TimescaleDB to be ready..."
kubectl -n adsb-tracker wait --for=condition=ready pod -l app=timescaledb --timeout=120s || true

# Deploy Rust DSP Processor
echo ""
echo "[4/6] Deploying Rust DSP Processor..."
kubectl apply -f rust-dsp/deployment.yaml
kubectl apply -f rust-dsp/service.yaml

# Deploy WebSocket Gateway
echo ""
echo "[5/6] Deploying WebSocket Gateway..."
kubectl apply -f websocket-gateway/deployment.yaml
kubectl apply -f websocket-gateway/service.yaml

# Deploy RTL-SDR Capture (DaemonSet)
echo ""
echo "[6/6] Deploying RTL-SDR Capture..."
kubectl apply -f rtl-sdr-capture/deployment.yaml
kubectl apply -f rtl-sdr-capture/service.yaml

echo ""
echo "=========================================="
echo "   Deployment Complete!"
echo "=========================================="
echo ""
echo "Check deployment status:"
echo "  kubectl -n adsb-tracker get pods"
echo ""
echo "Access the web UI:"
echo "  kubectl -n adsb-tracker port-forward svc/websocket-gateway 8888:8888"
echo "  Then open http://localhost:8888"
echo ""
echo "Or via NodePort:"
echo "  http://localhost:30888"
