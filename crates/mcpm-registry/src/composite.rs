use async_trait::async_trait;
use mcpm_core::{McpmError, ModVersion, Project, Registry, SourceKind};

use crate::{CurseForgeClient, ModrinthClient};

/// Routes registry calls to the appropriate source client and merges
/// search results across enabled sources.
pub struct CompositeRegistry {
    modrinth: ModrinthClient,
    curseforge: Option<CurseForgeClient>,
}

impl CompositeRegistry {
    pub fn new(modrinth: ModrinthClient, curseforge: Option<CurseForgeClient>) -> Self {
        Self {
            modrinth,
            curseforge,
        }
    }

    pub fn modrinth(&self) -> &ModrinthClient {
        &self.modrinth
    }

    pub fn curseforge(&self) -> Option<&CurseForgeClient> {
        self.curseforge.as_ref()
    }

    pub fn has_curseforge(&self) -> bool {
        self.curseforge.as_ref().is_some_and(|c| c.has_api_key())
    }
}

#[async_trait]
impl Registry for CompositeRegistry {
    async fn search_projects(&self, query: &str) -> Result<Vec<Project>, McpmError> {
        let mut results = self.modrinth.search_projects(query).await?;

        if let Some(curseforge) = &self.curseforge {
            if curseforge.has_api_key() {
                match curseforge.search_projects(query).await {
                    Ok(mut cf_results) => results.append(&mut cf_results),
                    Err(e) => tracing::warn!("CurseForge search failed: {}", e),
                }
            }
        }

        Ok(results)
    }

    async fn get_project(&self, source: SourceKind, id: &str) -> Result<Project, McpmError> {
        match source {
            SourceKind::Modrinth => self.modrinth.get_project(source, id).await,
            SourceKind::CurseForge => match &self.curseforge {
                Some(client) => client.get_project(source, id).await,
                None => Err(McpmError::Registry(
                    "CurseForge API key is not configured".to_string(),
                )),
            },
            SourceKind::Local => Err(McpmError::Registry(
                "local projects are not served by a registry".to_string(),
            )),
        }
    }

    async fn get_versions(
        &self,
        source: SourceKind,
        project_id: &str,
    ) -> Result<Vec<ModVersion>, McpmError> {
        match source {
            SourceKind::Modrinth => self.modrinth.get_versions(source, project_id).await,
            SourceKind::CurseForge => match &self.curseforge {
                Some(client) => client.get_versions(source, project_id).await,
                None => Err(McpmError::Registry(
                    "CurseForge API key is not configured".to_string(),
                )),
            },
            SourceKind::Local => Err(McpmError::Registry(
                "local projects are not served by a registry".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubRegistry {
        source: SourceKind,
    }

    #[async_trait]
    impl Registry for StubRegistry {
        async fn search_projects(&self, _query: &str) -> Result<Vec<Project>, McpmError> {
            Ok(vec![])
        }

        async fn get_project(&self, _source: SourceKind, id: &str) -> Result<Project, McpmError> {
            Ok(Project {
                id: id.to_string(),
                slug: None,
                title: id.to_string(),
                description: None,
                source: self.source,
                client_side: mcpm_core::Side::Both,
                server_side: mcpm_core::Side::Both,
                website_url: None,
            })
        }

        async fn get_versions(
            &self,
            _source: SourceKind,
            _project_id: &str,
        ) -> Result<Vec<ModVersion>, McpmError> {
            Ok(vec![])
        }
    }

    #[test]
    fn test_stub_source_roundtrip() {
        let stub = StubRegistry {
            source: SourceKind::CurseForge,
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let project = rt
            .block_on(stub.get_project(SourceKind::CurseForge, "1"))
            .unwrap();
        assert_eq!(project.source, SourceKind::CurseForge);
    }
}
