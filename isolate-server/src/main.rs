//! Isolate gRPC Server
//!
//! Provides remote sandbox management via gRPC.

mod service;

use clap::Parser;
use std::net::SocketAddr;
use tonic::transport::Server;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod proto {
    tonic::include_proto!("isolate.v1");
}

use proto::isolate_service_server::IsolateServiceServer;
use service::IsolateServiceImpl;

/// Isolate gRPC Server
#[derive(Parser, Debug)]
#[command(name = "isolate-server")]
#[command(about = "gRPC server for the Isolate secure sandbox runtime")]
struct Args {
    /// Address to bind to
    #[arg(short, long, default_value = "0.0.0.0:50051")]
    addr: SocketAddr,

    /// Log level
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Enable JSON logging
    #[arg(long)]
    json_logs: bool,

    /// Maximum number of concurrent sandboxes
    #[arg(long, default_value = "100")]
    max_sandboxes: usize,

    /// Enable warm pool
    #[arg(long)]
    warm_pool: bool,

    /// Warm pool size per module
    #[arg(long, default_value = "5")]
    warm_pool_size: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize logging
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| args.log_level.parse().unwrap_or_default());

    if args.json_logs {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
    }

    tracing::info!(addr = %args.addr, "Starting Isolate gRPC server");

    // Create the service
    let service = IsolateServiceImpl::new(args.max_sandboxes);

    // Start the server
    Server::builder()
        .add_service(IsolateServiceServer::new(service))
        .serve(args.addr)
        .await?;

    Ok(())
}
