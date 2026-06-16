use std::str::FromStr;

use agentik_skill::Skill;
use agentik_skill_proto::skill_registry::{
    skill_registry_service_client::SkillRegistryServiceClient, GetSkillRequest, ListSkillsRequest,
    ReloadSkillRequest,
};
use tonic::transport::Endpoint;

#[derive(Debug, thiserror::Error)]
pub enum SkillClientError {
    #[error("invalid address: {0}")]
    InvalidAddress(String),
    #[error("gRPC connection error: {0}")]
    Connection(#[from] tonic::transport::Error),
    #[error("gRPC status error: {0}")]
    Status(#[from] tonic::Status),
    #[error("skill not found: {name}")]
    NotFound { name: String },
}

/// gRPC-based skill registry client.
///
/// Connects to a remote `agentik-skill-server` process and provides
/// lookup / list / reload operations.
pub struct SkillRegistryClient {
    inner: SkillRegistryServiceClient<tonic::transport::Channel>,
}

impl SkillRegistryClient {
    /// Connect to a skill registry server at the given endpoint (e.g. "http://127.0.0.1:50051").
    pub async fn connect(addr: &str) -> Result<Self, SkillClientError> {
        let endpoint = Endpoint::from_str(addr)
            .map_err(|e| SkillClientError::InvalidAddress(e.to_string()))?;
        let channel = endpoint.connect().await?;
        Ok(Self {
            inner: SkillRegistryServiceClient::new(channel),
        })
    }

    /// Get a skill by name or alias.
    pub async fn get_skill(&mut self, name: &str) -> Result<Skill, SkillClientError> {
        let response = self
            .inner
            .get_skill(GetSkillRequest {
                name: name.to_string(),
            })
            .await?
            .into_inner();

        match response.result {
            Some(agentik_skill_proto::skill_registry::get_skill_response::Result::Skill(proto)) => {
                proto_to_skill(&proto)
            }
            Some(agentik_skill_proto::skill_registry::get_skill_response::Result::Error(_)) => {
                Err(SkillClientError::NotFound { name: name.to_string() })
            }
            None => Err(SkillClientError::NotFound { name: name.to_string() }),
        }
    }

    /// List all skills, with optional filters.
    pub async fn list_skills(
        &mut self,
        user_invocable_only: bool,
        model_invocable_only: bool,
    ) -> Result<Vec<Skill>, SkillClientError> {
        let response = self
            .inner
            .list_skills(ListSkillsRequest {
                user_invocable_only,
                model_invocable_only,
            })
            .await?
            .into_inner();

        response
            .skills
            .iter()
            .map(proto_to_skill)
            .collect()
    }

    /// Reload a skill from the server side.
    pub async fn reload_skill(&mut self, name: &str) -> Result<Option<Skill>, SkillClientError> {
        let response = self
            .inner
            .reload_skill(ReloadSkillRequest {
                name: name.to_string(),
            })
            .await?
            .into_inner();

        match response.result {
            Some(agentik_skill_proto::skill_registry::reload_skill_response::Result::Skill(
                proto,
            )) => Ok(Some(proto_to_skill(&proto)?)),
            Some(
                agentik_skill_proto::skill_registry::reload_skill_response::Result::NotChanged(
                    _,
                ),
            ) => Ok(None),
            Some(agentik_skill_proto::skill_registry::reload_skill_response::Result::Error(
                msg,
            )) => Err(SkillClientError::Status(tonic::Status::internal(msg))),
            None => Ok(None),
        }
    }
}

/// Convert a proto `SkillMessage` to a domain `Skill`.
fn proto_to_skill(
    proto: &agentik_skill_proto::skill_registry::SkillMessage,
) -> Result<Skill, SkillClientError> {
    let metadata = proto.metadata.as_ref().ok_or_else(|| {
        SkillClientError::Status(tonic::Status::internal("missing metadata".to_string()))
    })?;
    let policy = proto.policy.as_ref().ok_or_else(|| {
        SkillClientError::Status(tonic::Status::internal("missing policy".to_string()))
    })?;

    Ok(Skill {
        metadata: agentik_skill::SkillMetadata {
            name: metadata.name.clone(),
            description: metadata.description.clone(),
            aliases: metadata.aliases.clone(),
            when_to_use: metadata.when_to_use.clone(),
            argument_hint: metadata.argument_hint.clone(),
            user_invocable: metadata.user_invocable,
            model_invocable: metadata.model_invocable,
        },
        policy: agentik_skill::SkillPolicy {
            allowed_tools: policy.allowed_tools.iter().cloned().collect(),
        },
        body: proto.body.clone(),
        references: proto
            .references
            .iter()
            .map(|r| agentik_skill::ReferenceFile {
                name: r.name.clone(),
                content: r.content.clone(),
            })
            .collect(),
        activation_paths: proto.activation_paths.clone(),
        skill_dir: std::path::PathBuf::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proto_to_skill_roundtrip() {
        let proto = agentik_skill_proto::skill_registry::SkillMessage {
            metadata: Some(agentik_skill_proto::skill_registry::SkillMetadata {
                name: "test".to_string(),
                description: "A test skill".to_string(),
                aliases: vec!["t".to_string()],
                when_to_use: Some("when testing".to_string()),
                argument_hint: None,
                user_invocable: true,
                model_invocable: false,
            }),
            policy: Some(agentik_skill_proto::skill_registry::SkillPolicy {
                allowed_tools: vec!["bash".to_string(), "read".to_string()],
            }),
            body: "Do the thing.".to_string(),
            references: vec![agentik_skill_proto::skill_registry::ReferenceFile {
                name: "ref.md".to_string(),
                content: "ref content".to_string(),
            }],
            activation_paths: vec!["src/**".to_string()],
        };

        let skill = proto_to_skill(&proto).unwrap();
        assert_eq!(skill.metadata.name, "test");
        assert_eq!(skill.metadata.aliases, vec!["t"]);
        assert_eq!(skill.policy.allowed_tools.len(), 2);
        assert_eq!(skill.body, "Do the thing.");
        assert_eq!(skill.references.len(), 1);
        assert_eq!(skill.activation_paths, vec!["src/**"]);
        assert!(skill.skill_dir.as_os_str().is_empty());
    }
}
