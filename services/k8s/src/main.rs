mod cluster;
mod config;
mod resource;
mod service;

use ecat::App;
use ecat_transport_grpc::GrpcServer;
use superops_protos::k8s::v1::k8s_service_server::K8sServiceServer;

use crate::cluster::manager::ClusterManager;
use crate::config::Config;
use crate::service::K8sServiceImpl;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    let manager = ClusterManager::new();

    let grpc = GrpcServer::new(format!("0.0.0.0:{}", config.server.grpc_port)).routes(
        tonic::service::Routes::new(K8sServiceServer::new(K8sServiceImpl { manager })),
    );

    let mut app = App::builder()
        .name("superops-k8s")
        .version(env!("CARGO_PKG_VERSION"))
        .server(grpc)
        .build()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    app.run().await.map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(())
}
