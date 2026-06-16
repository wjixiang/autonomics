pub mod fs_store;
pub mod grpc_service;
pub mod registry;
pub mod store;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::fs_store::FilesystemSkillStore;
use crate::grpc_service::SkillRegistryGrpcService;
use crate::registry::SkillRegistry;
use crate::store::SkillStore;

/// Start the skill registry gRPC server.
///
/// Can be called from the main binary or embedded as a background task.
pub async fn run(
    addr: SocketAddr,
    skill_dirs: Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing::info!(%addr, ?skill_dirs, "starting skill registry server");

    let store = FilesystemSkillStore::new(skill_dirs).await?;
    let change_rx = store.subscribe();
    let store: Arc<dyn SkillStore> = Arc::new(store);

    let registry = Arc::new(SkillRegistry::new(store));
    let grpc_service = SkillRegistryGrpcService::new(registry, change_rx);

    tonic::transport::Server::builder()
        .add_service(grpc_service.into_server())
        .serve(addr)
        .await?;

    Ok(())
}
