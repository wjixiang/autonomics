use std::sync::Arc;

use agentik_skill::Skill;
use agentik_skill_proto::skill_registry::{
    get_skill_response::Result as GetResult,
    get_skill_tree_response::Result as TreeResult,
    reload_skill_response::Result as ReloadResult,
    skill_change_event::ChangeType as ProtoChangeType,
    skill_registry_service_server::SkillRegistryServiceServer,
    GetSkillResponse, GetSkillTreeRequest, GetSkillTreeResponse,
    ListSkillsRequest, ListSkillsResponse, ReloadSkillResponse,
    SkillChangeEvent, SkillMessage as ProtoSkill, SkillMetadata as ProtoMetadata,
    SkillPolicy as ProtoPolicy, ReferenceFile as ProtoRef,
};
use agentik_skill_proto::skill_registry::skill_registry_service_server::SkillRegistryService;
use tonic::{Request, Response, Status};

use crate::fs_store::{SkillChangeNotification, SkillChangeType};
use crate::registry::SkillRegistry;

pub struct SkillRegistryGrpcService {
    registry: Arc<SkillRegistry>,
    change_rx: tokio::sync::broadcast::Receiver<SkillChangeNotification>,
}

impl SkillRegistryGrpcService {
    pub fn new(
        registry: Arc<SkillRegistry>,
        change_rx: tokio::sync::broadcast::Receiver<SkillChangeNotification>,
    ) -> Self {
        Self { registry, change_rx }
    }

    pub fn into_server(self) -> SkillRegistryServiceServer<Self> {
        SkillRegistryServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl SkillRegistryService for SkillRegistryGrpcService {
    type WatchSkillsStream =
        std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<SkillChangeEvent, Status>> + Send>>;

    async fn get_skill(
        &self,
        request: Request<agentik_skill_proto::skill_registry::GetSkillRequest>,
    ) -> Result<Response<GetSkillResponse>, Status> {
        let name = &request.into_inner().name;
        match self.registry.get_skill(name).await {
            Ok(skill) => Ok(Response::new(GetSkillResponse {
                result: Some(GetResult::Skill(skill_to_proto(&skill))),
            })),
            Err(e) => Ok(Response::new(GetSkillResponse {
                result: Some(GetResult::Error(e.to_string())),
            })),
        }
    }

    async fn list_skills(
        &self,
        request: Request<ListSkillsRequest>,
    ) -> Result<Response<ListSkillsResponse>, Status> {
        let req = request.into_inner();
        let skills = self
            .registry
            .list_skills(req.user_invocable_only, req.model_invocable_only)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(ListSkillsResponse {
            skills: skills.iter().map(skill_to_proto).collect(),
        }))
    }

    async fn reload_skill(
        &self,
        request: Request<agentik_skill_proto::skill_registry::ReloadSkillRequest>,
    ) -> Result<Response<ReloadSkillResponse>, Status> {
        let name = &request.into_inner().name;
        match self.registry.reload_skill(name).await {
            Ok(Some(skill)) => Ok(Response::new(ReloadSkillResponse {
                result: Some(ReloadResult::Skill(skill_to_proto(&skill))),
            })),
            Ok(None) => Ok(Response::new(ReloadSkillResponse {
                result: Some(ReloadResult::NotChanged(
                    "skill is already up to date".to_string(),
                )),
            })),
            Err(e) => Ok(Response::new(ReloadSkillResponse {
                result: Some(ReloadResult::Error(e.to_string())),
            })),
        }
    }

    async fn watch_skills(
        &self,
        _request: Request<agentik_skill_proto::skill_registry::WatchSkillsRequest>,
    ) -> Result<Response<Self::WatchSkillsStream>, Status> {
        use tokio_stream::wrappers::BroadcastStream;
        use tokio_stream::StreamExt;

        let stream = BroadcastStream::new(self.change_rx.resubscribe()).filter_map(|result| {
            result.ok().map(|notif| {
                Ok(SkillChangeEvent {
                    change_type: match notif.change_type {
                        SkillChangeType::Added => ProtoChangeType::Added as i32,
                        SkillChangeType::Modified => ProtoChangeType::Modified as i32,
                        SkillChangeType::Removed => ProtoChangeType::Removed as i32,
                    },
                    skill_name: notif.skill_name,
                    skill: None,
                })
            })
        });

        Ok(Response::new(Box::pin(stream)))
    }

    async fn get_skill_tree(
        &self,
        _request: Request<GetSkillTreeRequest>,
    ) -> Result<Response<GetSkillTreeResponse>, Status> {
        match self.registry.get_root_skill().await {
            Ok(root) => Ok(Response::new(GetSkillTreeResponse {
                result: Some(TreeResult::Root(skill_to_proto(&root))),
            })),
            Err(e) => Ok(Response::new(GetSkillTreeResponse {
                result: Some(TreeResult::Error(e.to_string())),
            })),
        }
    }
}

/// Convert a domain `Skill` to a proto `SkillMessage`.
fn skill_to_proto(skill: &Skill) -> ProtoSkill {
    ProtoSkill {
        metadata: Some(ProtoMetadata {
            name: skill.metadata.name.clone(),
            description: skill.metadata.description.clone(),
            aliases: skill.metadata.aliases.clone(),
            when_to_use: skill.metadata.when_to_use.clone(),
            argument_hint: skill.metadata.argument_hint.clone(),
            user_invocable: skill.metadata.user_invocable,
            model_invocable: skill.metadata.model_invocable,
        }),
        policy: Some(ProtoPolicy {
            allowed_tools: skill.policy.allowed_tools.iter().cloned().collect(),
        }),
        body: skill.body.clone(),
        references: skill
            .references
            .iter()
            .map(|r| ProtoRef {
                name: r.name.clone(),
                content: r.content.clone(),
            })
            .collect(),
        activation_paths: skill.activation_paths.clone(),
    }
}
