use async_trait::async_trait;
use mcpm_core::{
    Dependency, DependencyKind, FileArtifact, Hashes, LoaderKind, McpmError, ModVersion, Project,
    Registry, ReleaseType, Side, SourceKind,
};
use reqwest::Client;
use serde::Deserialize;

const CURSEFORGE_API_BASE: &str = "https://api.curseforge.com/v1";
const CURSEFORGE_USER_AGENT: &str = "mcpm/0.1.0 (github.com/mcpm)";
const MINECRAFT_GAME_ID: u32 = 432;
/// CurseForge class id for Minecraft mods.
const MODS_CLASS_ID: u32 = 6;

pub struct CurseForgeClient {
    client: Client,
    api_key: String,
    base_url: String,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: Vec<CfProject>,
}

#[derive(Debug, Deserialize)]
struct CfProject {
    id: u64,
    name: String,
    slug: Option<String>,
    summary: Option<String>,
    #[serde(default)]
    links: Option<CfLinks>,
}

#[derive(Debug, Deserialize)]
struct CfLinks {
    #[serde(default, rename = "websiteUrl")]
    website_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CfFile {
    id: u64,
    #[serde(rename = "modId")]
    mod_id: u64,
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
    #[serde(rename = "fileName")]
    file_name: String,
    #[serde(default)]
    #[serde(rename = "releaseType")]
    release_type: Option<u32>,
    #[serde(default)]
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
    #[serde(default)]
    #[serde(rename = "gameVersions")]
    game_versions: Vec<String>,
    #[serde(default)]
    dependencies: Vec<CfDependency>,
    #[serde(default)]
    hashes: Vec<CfHash>,
    #[serde(default)]
    #[serde(rename = "fileLength")]
    file_length: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct CfDependency {
    #[serde(rename = "modId")]
    mod_id: u64,
    #[serde(rename = "relationType")]
    relation_type: u32,
}

#[derive(Debug, Deserialize)]
struct CfHash {
    #[serde(default)]
    value: String,
    #[serde(default)]
    algo: u32,
}

impl CurseForgeClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        let client = Client::builder()
            .user_agent(CURSEFORGE_USER_AGENT)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            api_key: api_key.into(),
            base_url: CURSEFORGE_API_BASE.to_string(),
        }
    }

    pub fn with_base_url(api_key: impl Into<String>, base_url: &str) -> Self {
        let client = Client::builder()
            .user_agent(CURSEFORGE_USER_AGENT)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            api_key: api_key.into(),
            base_url: base_url.to_string(),
        }
    }

    pub fn has_api_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        what: &str,
    ) -> Result<T, McpmError> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .header("x-api-key", &self.api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| McpmError::Registry(format!("CurseForge {} failed: {}", what, e)))?;

        if resp.status() == reqwest::StatusCode::FORBIDDEN
            || resp.status() == reqwest::StatusCode::UNAUTHORIZED
        {
            return Err(McpmError::Registry(format!(
                "CurseForge {} rejected the API key (HTTP {})",
                what,
                resp.status()
            )));
        }
        if !resp.status().is_success() {
            return Err(McpmError::Registry(format!(
                "CurseForge {} returned status {}",
                what,
                resp.status()
            )));
        }

        resp.json()
            .await
            .map_err(|e| McpmError::Registry(format!("failed to parse CurseForge {}: {}", what, e)))
    }

    /// Resolves a download URL for a file, falling back to the dedicated
    /// download-url endpoint when the file's `downloadUrl` is null.
    pub async fn resolve_download_url(
        &self,
        mod_id: &str,
        file_id: &str,
        direct: Option<&str>,
    ) -> Option<String> {
        if let Some(url) = direct {
            if !url.is_empty() {
                return Some(url.to_string());
            }
        }
        let path = format!("/mods/{}/files/{}/download-url", mod_id, file_id);
        self.get_json::<ApiResponse<String>>(&path, "download-url")
            .await
            .ok()
            .map(|r| r.data)
    }
}

fn parse_loader_token(token: &str) -> Option<LoaderKind> {
    match token.to_lowercase().as_str() {
        "fabric" => Some(LoaderKind::Fabric),
        "quilt" => Some(LoaderKind::Quilt),
        "forge" => Some(LoaderKind::Forge),
        "neoforge" => Some(LoaderKind::NeoForge),
        _ => None,
    }
}

fn split_game_versions(game_versions: &[String]) -> (Vec<String>, Vec<LoaderKind>) {
    let mut minecraft = Vec::new();
    let mut loaders = Vec::new();
    for token in game_versions {
        if let Some(loader) = parse_loader_token(token) {
            if !loaders.contains(&loader) {
                loaders.push(loader);
            }
        } else {
            minecraft.push(token.clone());
        }
    }
    (minecraft, loaders)
}

fn parse_release_type(value: Option<u32>) -> ReleaseType {
    match value {
        Some(2) => ReleaseType::Beta,
        Some(3) => ReleaseType::Alpha,
        _ => ReleaseType::Release,
    }
}

fn parse_relation(relation_type: u32) -> Option<DependencyKind> {
    match relation_type {
        3 => Some(DependencyKind::Required),
        2 => Some(DependencyKind::Optional),
        5 => Some(DependencyKind::Incompatible),
        _ => None,
    }
}

fn convert_file(file: CfFile) -> ModVersion {
    let (minecraft, loaders) = split_game_versions(&file.game_versions);

    let sha1 = file
        .hashes
        .iter()
        .find(|h| h.algo == 1)
        .map(|h| h.value.clone());

    let dependencies = file
        .dependencies
        .iter()
        .filter_map(|dep| {
            parse_relation(dep.relation_type).map(|kind| Dependency {
                project_id: Some(dep.mod_id.to_string()),
                version_id: None,
                kind,
            })
        })
        .collect();

    ModVersion {
        id: file.id.to_string(),
        project_id: file.mod_id.to_string(),
        name: file.display_name.clone(),
        version_number: file
            .display_name
            .clone()
            .unwrap_or_else(|| file.file_name.clone()),
        version_type: parse_release_type(file.release_type),
        game_versions: minecraft,
        loaders,
        files: vec![FileArtifact {
            filename: file.file_name,
            url: file.download_url.clone().unwrap_or_default(),
            hashes: Hashes { sha512: None, sha1 },
            size: file.file_length,
            primary: true,
        }],
        dependencies,
    }
}

#[async_trait]
impl Registry for CurseForgeClient {
    async fn search_projects(&self, query: &str) -> Result<Vec<Project>, McpmError> {
        let path = format!(
            "/mods/search?gameId={}&classId={}&searchFilter={}&pageSize=10",
            MINECRAFT_GAME_ID,
            MODS_CLASS_ID,
            urlencode(query)
        );
        let response: SearchResponse = self.get_json(&path, "search").await?;
        Ok(response.data.into_iter().map(convert_project).collect())
    }

    async fn get_project(&self, source: SourceKind, id: &str) -> Result<Project, McpmError> {
        if source != SourceKind::CurseForge {
            return Err(McpmError::Registry(format!(
                "CurseForge client cannot serve source '{}'",
                source
            )));
        }
        let path = format!("/mods/{}", id);
        let response: ApiResponse<CfProject> = self.get_json(&path, "project").await?;
        Ok(convert_project(response.data))
    }

    async fn get_versions(
        &self,
        source: SourceKind,
        project_id: &str,
    ) -> Result<Vec<ModVersion>, McpmError> {
        if source != SourceKind::CurseForge {
            return Err(McpmError::Registry(format!(
                "CurseForge client cannot serve source '{}'",
                source
            )));
        }
        let path = format!("/mods/{}/files?pageSize=50", project_id);
        let response: ApiResponse<Vec<CfFile>> = self.get_json(&path, "files").await?;
        Ok(response.data.into_iter().map(convert_file).collect())
    }
}

fn convert_project(project: CfProject) -> Project {
    Project {
        id: project.id.to_string(),
        slug: project.slug,
        title: project.name,
        description: project.summary,
        source: SourceKind::CurseForge,
        client_side: Side::Both,
        server_side: Side::Both,
        website_url: project.links.and_then(|l| l.website_url),
    }
}

fn urlencode(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => result.push_str(&format!("%{:02X}", byte)),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_game_versions() {
        let (mc, loaders) = split_game_versions(&[
            "1.20.1".to_string(),
            "Forge".to_string(),
            "NeoForge".to_string(),
        ]);
        assert_eq!(mc, vec!["1.20.1".to_string()]);
        assert_eq!(loaders, vec![LoaderKind::Forge, LoaderKind::NeoForge]);
    }

    #[test]
    fn test_convert_file_maps_fields() {
        let file = CfFile {
            id: 5000000,
            mod_id: 238222,
            display_name: Some("jei-1.20.1-15.2.0.27.jar".to_string()),
            file_name: "jei-1.20.1-15.2.0.27.jar".to_string(),
            release_type: Some(1),
            download_url: Some("https://edge.forgecdn.net/files/5000000/jei.jar".to_string()),
            game_versions: vec!["1.20.1".to_string(), "Forge".to_string()],
            dependencies: vec![CfDependency {
                mod_id: 111,
                relation_type: 3,
            }],
            hashes: vec![CfHash {
                value: "abc123".to_string(),
                algo: 1,
            }],
            file_length: Some(1234),
        };

        let version = convert_file(file);
        assert_eq!(version.id, "5000000");
        assert_eq!(version.project_id, "238222");
        assert_eq!(version.game_versions, vec!["1.20.1".to_string()]);
        assert_eq!(version.loaders, vec![LoaderKind::Forge]);
        assert_eq!(version.files[0].hashes.sha1.as_deref(), Some("abc123"));
        assert_eq!(version.dependencies.len(), 1);
        assert_eq!(version.dependencies[0].kind, DependencyKind::Required);
    }

    #[test]
    fn test_parse_relation_types() {
        assert_eq!(parse_relation(3), Some(DependencyKind::Required));
        assert_eq!(parse_relation(2), Some(DependencyKind::Optional));
        assert_eq!(parse_relation(5), Some(DependencyKind::Incompatible));
        assert_eq!(parse_relation(1), None);
    }

    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_wiremock_get_project_and_versions() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/mods/238222"))
            .and(header("x-api-key", "test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "id": 238222,
                    "name": "Just Enough Items",
                    "slug": "jei",
                    "summary": "View items",
                    "links": { "websiteUrl": "https://www.curseforge.com/minecraft/mc-mods/jei" }
                }
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/mods/238222/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "id": 5000000,
                    "modId": 238222,
                    "displayName": "jei-1.20.1-15.2.0.27.jar",
                    "fileName": "jei-1.20.1-15.2.0.27.jar",
                    "releaseType": 1,
                    "downloadUrl": "https://edge.forgecdn.net/files/5000000/jei.jar",
                    "gameVersions": ["1.20.1", "Forge"],
                    "dependencies": [],
                    "hashes": [{ "value": "abc", "algo": 1 }],
                    "fileLength": 100
                }]
            })))
            .mount(&server)
            .await;

        let client = CurseForgeClient::with_base_url("test-key", &server.uri());

        let project = client
            .get_project(SourceKind::CurseForge, "238222")
            .await
            .unwrap();
        assert_eq!(project.title, "Just Enough Items");
        assert_eq!(project.source, SourceKind::CurseForge);
        assert_eq!(
            project.website_url.as_deref(),
            Some("https://www.curseforge.com/minecraft/mc-mods/jei")
        );

        let versions = client
            .get_versions(SourceKind::CurseForge, "238222")
            .await
            .unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].id, "5000000");
        assert_eq!(versions[0].loaders, vec![LoaderKind::Forge]);
    }

    #[tokio::test]
    async fn test_wiremock_download_url_fallback() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/mods/1/files/2/download-url"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "data": "https://cdn.example/x.jar" })),
            )
            .mount(&server)
            .await;

        let client = CurseForgeClient::with_base_url("test-key", &server.uri());
        let url = client.resolve_download_url("1", "2", None).await;
        assert_eq!(url.as_deref(), Some("https://cdn.example/x.jar"));
    }

    #[tokio::test]
    async fn test_wiremock_search() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/mods/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "id": 238222,
                    "name": "Just Enough Items",
                    "slug": "jei",
                    "summary": "View items"
                }]
            })))
            .mount(&server)
            .await;

        let client = CurseForgeClient::with_base_url("test-key", &server.uri());
        let results = client.search_projects("jei").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "238222");
    }
}
