# PowerShell deployment script for ADS-B Tracker on Kubernetes
# Run from project root: .\scripts\deploy.ps1

param(
    [switch]$Build,
    [switch]$Deploy,
    [switch]$All,
    [switch]$Status,
    [switch]$Logs,
    [switch]$Delete
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

function Write-Header($text) {
    Write-Host ""
    Write-Host "==========================================" -ForegroundColor Cyan
    Write-Host "   $text" -ForegroundColor Cyan
    Write-Host "==========================================" -ForegroundColor Cyan
}

function Build-Images {
    Write-Header "Building Docker Images"

    Set-Location $ProjectRoot

    Write-Host "`n[1/3] Building Rust DSP Processor..." -ForegroundColor Yellow
    docker build -t adsb-tracker/rust-dsp:latest -f services/rust-dsp/Dockerfile .
    if ($LASTEXITCODE -ne 0) { throw "Failed to build rust-dsp" }

    Write-Host "`n[2/3] Building WebSocket Gateway..." -ForegroundColor Yellow
    docker build -t adsb-tracker/websocket-gateway:latest -f services/websocket-gateway/Dockerfile .
    if ($LASTEXITCODE -ne 0) { throw "Failed to build websocket-gateway" }

    Write-Host "`n[3/3] Building RTL-SDR Capture (optional)..." -ForegroundColor Yellow
    docker build -t adsb-tracker/rtl-sdr-capture:latest -f services/rtl-sdr-capture/Dockerfile . 2>$null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "  RTL-SDR build skipped (requires librtlsdr)" -ForegroundColor DarkYellow
    }

    Write-Host "`nImages built:" -ForegroundColor Green
    docker images | Select-String "adsb-tracker"
}

function Deploy-ToK8s {
    Write-Header "Deploying to Kubernetes"

    # Check kubectl
    if (-not (Get-Command kubectl -ErrorAction SilentlyContinue)) {
        throw "kubectl not found. Please install kubectl."
    }

    Write-Host "Current context: $(kubectl config current-context)" -ForegroundColor Yellow

    Set-Location "$ProjectRoot\k8s"

    Write-Host "`n[1/6] Creating namespace..." -ForegroundColor Yellow
    kubectl apply -f namespace.yaml

    Write-Host "`n[2/6] Creating ConfigMap and Secret..." -ForegroundColor Yellow
    kubectl apply -f configmap.yaml
    kubectl apply -f secret.yaml

    Write-Host "`n[3/6] Deploying TimescaleDB..." -ForegroundColor Yellow
    kubectl apply -f timescaledb/init-configmap.yaml
    kubectl apply -f timescaledb/statefulset.yaml
    kubectl apply -f timescaledb/service.yaml

    Write-Host "  Waiting for TimescaleDB..." -ForegroundColor DarkYellow
    kubectl -n adsb-tracker wait --for=condition=ready pod -l app=timescaledb --timeout=120s 2>$null

    Write-Host "`n[4/6] Deploying Rust DSP Processor..." -ForegroundColor Yellow
    kubectl apply -f rust-dsp/deployment.yaml
    kubectl apply -f rust-dsp/service.yaml

    Write-Host "`n[5/6] Deploying WebSocket Gateway..." -ForegroundColor Yellow
    kubectl apply -f websocket-gateway/deployment.yaml
    kubectl apply -f websocket-gateway/service.yaml

    Write-Host "`n[6/6] Deploying RTL-SDR Capture..." -ForegroundColor Yellow
    kubectl apply -f rtl-sdr-capture/deployment.yaml
    kubectl apply -f rtl-sdr-capture/service.yaml

    Write-Header "Deployment Complete!"
    Write-Host ""
    Write-Host "Check status:  .\scripts\deploy.ps1 -Status" -ForegroundColor White
    Write-Host "View logs:     .\scripts\deploy.ps1 -Logs" -ForegroundColor White
    Write-Host ""
    Write-Host "Access Web UI: http://localhost:30888" -ForegroundColor Green
}

function Show-Status {
    Write-Header "Deployment Status"

    Write-Host "`nPods:" -ForegroundColor Yellow
    kubectl -n adsb-tracker get pods -o wide

    Write-Host "`nServices:" -ForegroundColor Yellow
    kubectl -n adsb-tracker get services

    Write-Host "`nEndpoints:" -ForegroundColor Yellow
    kubectl -n adsb-tracker get endpoints
}

function Show-Logs {
    Write-Header "Viewing Logs (Ctrl+C to exit)"

    $pod = Read-Host "Enter pod name (or 'all' for all pods)"

    if ($pod -eq "all") {
        kubectl -n adsb-tracker logs -f -l app --all-containers=true
    } else {
        kubectl -n adsb-tracker logs -f $pod
    }
}

function Delete-Deployment {
    Write-Header "Deleting Deployment"

    $confirm = Read-Host "Are you sure you want to delete the adsb-tracker namespace? (yes/no)"
    if ($confirm -eq "yes") {
        kubectl delete namespace adsb-tracker
        Write-Host "Namespace deleted." -ForegroundColor Green
    } else {
        Write-Host "Cancelled." -ForegroundColor Yellow
    }
}

# Main
if ($All -or ($Build -and $Deploy)) {
    Build-Images
    Deploy-ToK8s
} elseif ($Build) {
    Build-Images
} elseif ($Deploy) {
    Deploy-ToK8s
} elseif ($Status) {
    Show-Status
} elseif ($Logs) {
    Show-Logs
} elseif ($Delete) {
    Delete-Deployment
} else {
    Write-Host "ADS-B Tracker Deployment Script" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Usage: .\scripts\deploy.ps1 [options]"
    Write-Host ""
    Write-Host "Options:"
    Write-Host "  -Build    Build Docker images"
    Write-Host "  -Deploy   Deploy to Kubernetes"
    Write-Host "  -All      Build and deploy"
    Write-Host "  -Status   Show deployment status"
    Write-Host "  -Logs     View pod logs"
    Write-Host "  -Delete   Delete deployment"
    Write-Host ""
    Write-Host "Examples:"
    Write-Host "  .\scripts\deploy.ps1 -All      # Build and deploy everything"
    Write-Host "  .\scripts\deploy.ps1 -Status   # Check pod status"
}
