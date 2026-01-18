//! gRPC Gateway - receives streams from host and routes to WebSocket/DB

use anyhow::Result;
use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use tokio::sync::broadcast;
use tonic::transport::Server;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod db_writer;
mod grpc_server;
mod ws_handler;

use db_writer::DbWriter;
use grpc_server::GatewayService;

pub mod adsb {
    tonic::include_proto!("adsb");
}

/// Shared application state
pub struct AppState {
    pub db_writer: Arc<DbWriter>,
    pub broadcast_tx: Arc<broadcast::Sender<String>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("grpc_gateway=info".parse().unwrap())
            .add_directive("tower_http=info".parse().unwrap()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("===========================================");
    info!("   gRPC Gateway - ADS-B Flight Tracker");
    info!("===========================================");

    // Load configuration from environment
    let grpc_port: u16 = std::env::var("GRPC_PORT")
        .unwrap_or_else(|_| "50051".to_string())
        .parse()
        .unwrap_or(50051);

    let ws_port: u16 = std::env::var("WS_PORT")
        .unwrap_or_else(|_| "8888".to_string())
        .parse()
        .unwrap_or(8888);

    let db_host = std::env::var("DB_HOST").unwrap_or_else(|_| "localhost".to_string());
    let db_port = std::env::var("DB_PORT").unwrap_or_else(|_| "5432".to_string());
    let db_name = std::env::var("DB_NAME").unwrap_or_else(|_| "adsb".to_string());
    let db_user = std::env::var("DB_USER").unwrap_or_else(|_| "adsb".to_string());
    let db_password = std::env::var("DB_PASSWORD").unwrap_or_else(|_| "adsb".to_string());
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "/app/static".to_string());

    let db_url = format!(
        "host={} port={} dbname={} user={} password={}",
        db_host, db_port, db_name, db_user, db_password
    );

    info!("Configuration:");
    info!("  gRPC port: {}", grpc_port);
    info!("  HTTP/WS port: {}", ws_port);
    info!("  Database: {}@{}:{}/{}", db_user, db_host, db_port, db_name);
    info!("  Static files: {}", static_dir);

    // Create broadcast channel for WebSocket clients
    let (broadcast_tx, _) = broadcast::channel::<String>(1000);
    let broadcast_tx = Arc::new(broadcast_tx);

    // Connect to database
    let db_writer = match DbWriter::new(&db_url).await {
        Ok(db) => {
            info!("Connected to database");
            Arc::new(db)
        }
        Err(e) => {
            error!("Failed to connect to database: {}. Continuing without DB.", e);
            Arc::new(DbWriter::new_dummy())
        }
    };

    // Create shared app state
    let app_state = Arc::new(AppState {
        db_writer: db_writer.clone(),
        broadcast_tx: broadcast_tx.clone(),
    });

    // Create gRPC service
    let gateway_service = GatewayService::new(db_writer.clone(), broadcast_tx.clone());

    // Build HTTP/WebSocket router
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // WebSocket endpoint
        .route("/ws", get(ws_handler::ws_handler))
        // REST API endpoints
        .route("/api/aircraft", get(get_aircraft))
        .route("/api/aircraft/:icao/trail", get(get_aircraft_trail))
        .route("/api/sdr/status", get(get_sdr_status))
        .route("/health", get(health_check))
        // Static files
        .nest_service("/", ServeDir::new(&static_dir))
        .layer(cors)
        .with_state(app_state);

    // Start gRPC server
    let grpc_addr = format!("0.0.0.0:{}", grpc_port).parse()?;
    info!("Starting gRPC server on {}", grpc_addr);

    let grpc_server = Server::builder()
        .add_service(adsb::adsb_gateway_server::AdsbGatewayServer::new(gateway_service))
        .serve(grpc_addr);

    // Start HTTP/WebSocket server
    let http_addr = format!("0.0.0.0:{}", ws_port);
    info!("Starting HTTP/WebSocket server on {}", http_addr);

    let listener = tokio::net::TcpListener::bind(&http_addr).await?;
    let http_server = axum::serve(listener, app);

    // Run both servers concurrently
    tokio::select! {
        result = grpc_server => {
            if let Err(e) = result {
                error!("gRPC server error: {}", e);
            }
        }
        result = http_server => {
            if let Err(e) = result {
                error!("HTTP server error: {}", e);
            }
        }
    }

    Ok(())
}

/// Health check endpoint
async fn health_check() -> &'static str {
    "OK"
}

/// Query parameters for trail endpoint
#[derive(serde::Deserialize)]
struct TrailParams {
    minutes: Option<i32>,
}

/// Get current aircraft list
async fn get_aircraft(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db_writer.get_current_aircraft().await {
        Ok(aircraft) => Json(aircraft).into_response(),
        Err(e) => {
            error!("Failed to get aircraft: {}", e);
            Json(serde_json::json!({"error": e.to_string()})).into_response()
        }
    }
}

/// Get aircraft position trail
async fn get_aircraft_trail(
    State(state): State<Arc<AppState>>,
    Path(icao): Path<String>,
    Query(params): Query<TrailParams>,
) -> impl IntoResponse {
    let minutes = params.minutes.unwrap_or(30);
    match state.db_writer.get_aircraft_trail(&icao, minutes).await {
        Ok(trail) => Json(trail).into_response(),
        Err(e) => {
            error!("Failed to get trail for {}: {}", icao, e);
            Json(serde_json::json!({"error": e.to_string()})).into_response()
        }
    }
}

/// Get SDR device status
async fn get_sdr_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db_writer.get_sdr_status().await {
        Ok(status) => Json(status).into_response(),
        Err(e) => {
            error!("Failed to get SDR status: {}", e);
            Json(serde_json::json!({"error": e.to_string()})).into_response()
        }
    }
}
