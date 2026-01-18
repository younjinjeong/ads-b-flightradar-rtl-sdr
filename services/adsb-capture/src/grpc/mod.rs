//! gRPC client module

mod client;

pub use client::StreamingGatewayClient;

// Re-export protobuf types
pub mod adsb {
    tonic::include_proto!("adsb");
}
