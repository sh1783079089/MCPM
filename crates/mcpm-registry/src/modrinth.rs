use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use mcpm_core::{
    Dependency, DependencyKind, FileArtifact, Hashes, LoaderKind, LoaderVersion,
    LoaderVersionRegistry, McpmError, ModVersion, Project, Registry, ReleaseType, Side, SourceKind,
};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Deserialize;

const MODRINTH_API_BASE: &str = "https://api.modrinth.com/v2";
const MODRINTH_USER_AGENT: &str = "mcpm/0.1.0 (github.com/mcpm)";
const FABRIC_LOADER_META: &str = "https://meta.fabricmc.net/v2/versions/loader";
const QUILT_LOADER_META: &str = "https://meta.quiltmc.org/v3/versions/loader";
const FORGE_PROMOTIONS: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const NEOFORGE_MAVEN_METADATA: &str =
    "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";

pub struct ModrinthClient {
    client: Client,
    base_url: String,
}

impl ModrinthClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent(MODRINTH_USER_AGENT)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            base_url: MODRINTH_API_BASE.to_string(),
        }
    }

    pub fn with_base_url(base_url: &str) -> Self {
        let client = Client::builder()
            .user_agent(MODRINTH_USER_AGENT)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            base_url: base_url.to_string(),
        }
    }

    async fn get_json<T: DeserializeOwned>(&self, url: &str, what: &str) -> Result<T, McpmError> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| McpmError::Registry(format!("{} request failed: {}", what, e)))?;
        if !resp.status().is_success() {
            return Err(McpmError::Registry(format!(
                "{} returned status {}",
                what,
                resp.status()
            )));
        }
        resp.json()
            .await
            .map_err(|e| McpmError::Registry(format!("failed to parse {}: {}", what, e)))
    }

    /// Looks up a specific version by file hash (sha512 or sha1).
    /// Returns `None` when Modrinth has no matching file.
    pub async fn get_version_by_hash(
        &self,
        hash: &str,
        algorithm: &str,
    ) -> Result<Option<ModVersion>, McpmError> {
        let url = format!(
            "{}/version_file/{}?algorithm={}",
            self.base_url,
            url_encode(hash),
            url_encode(algorithm)
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| McpmError::Registry(format!("Modrinth hash lookup failed: {}", e)))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Err(McpmError::Registry(format!(
                "Modrinth hash lookup returned status {}",
                resp.status()
            )));
        }

        let version: ModrinthVersion = resp.json().await.map_err(|e| {
            McpmError::Registry(format!("Failed to parse Modrinth version response: {}", e))
        })?;
        Ok(Some(convert_version(version)))
    }

    async fn get_text(&self, url: &str, what: &str) -> Result<String, McpmError> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| McpmError::Registry(format!("{} request failed: {}", what, e)))?;
        if !resp.status().is_success() {
            return Err(McpmError::Registry(format!(
                "{} returned status {}",
                what,
                resp.status()
            )));
        }
        resp.text()
            .await
            .map_err(|e| McpmError::Registry(format!("failed to read {}: {}", what, e)))
    }
}

#[derive(Debug, Deserialize)]
struct FabricLoaderEntry {
    version: String,
    #[serde(default)]
    stable: bool,
}

#[derive(Debug, Deserialize)]
struct QuiltLoaderEntry {
    version: String,
}

#[derive(Debug, Deserialize)]
struct ForgePromotions {
    #[serde(default)]
    promos: HashMap<String, String>,
}

fn extract_xml_versions(xml: &str) -> Vec<String> {
    let mut versions = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<version>") {
        let after = &rest[start + "<version>".len()..];
        match after.find("</version>") {
            Some(end) => {
                versions.push(after[..end].trim().to_string());
                rest = &after[end + "</version>".len()..];
            }
            None => break,
        }
    }
    versions
}

#[async_trait]
impl LoaderVersionRegistry for ModrinthClient {
    async fn get_loader_versions(
        &self,
        loader: LoaderKind,
        minecraft: &str,
    ) -> Result<Vec<LoaderVersion>, McpmError> {
        match loader {
            LoaderKind::Fabric => {
                let entries: Vec<FabricLoaderEntry> = self
                    .get_json(FABRIC_LOADER_META, "Fabric loader metadata")
                    .await?;
                Ok(entries
                    .into_iter()
                    .map(|e| LoaderVersion {
                        version: e.version,
                        stable: e.stable,
                    })
                    .collect())
            }
            LoaderKind::Quilt => {
                let entries: Vec<QuiltLoaderEntry> = self
                    .get_json(QUILT_LOADER_META, "Quilt loader metadata")
                    .await?;
                Ok(entries
                    .into_iter()
                    .map(|e| LoaderVersion {
                        version: e.version,
                        stable: true,
                    })
                    .collect())
            }
            LoaderKind::Forge => {
                let promotions: ForgePromotions =
                    self.get_json(FORGE_PROMOTIONS, "Forge promotions").await?;
                let prefix = format!("{}-", minecraft);
                let mut seen = HashSet::new();
                let mut versions = Vec::new();
                for (key, version) in &promotions.promos {
                    if key.starts_with(&prefix) && seen.insert(version.clone()) {
                        versions.push(LoaderVersion {
                            version: version.clone(),
                            stable: true,
                        });
                    }
                }
                Ok(versions)
            }
            LoaderKind::NeoForge => {
                let xml = self
                    .get_text(NEOFORGE_MAVEN_METADATA, "NeoForge maven metadata")
                    .await?;
                Ok(extract_xml_versions(&xml)
                    .into_iter()
                    .map(|version| LoaderVersion {
                        stable: !version.contains("beta"),
                        version,
                    })
                    .collect())
            }
        }
    }
}

impl Default for ModrinthClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    hits: Vec<SearchHit>,
}

#[derive(Debug, Deserialize)]
struct SearchHit {
    project_id: String,
    slug: Option<String>,
    title: String,
    description: Option<String>,
    client_side: Option<String>,
    server_side: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthProject {
    id: String,
    slug: Option<String>,
    title: String,
    description: Option<String>,
    client_side: Option<String>,
    server_side: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthVersion {
    id: String,
    project_id: String,
    name: Option<String>,
    version_number: String,
    version_type: String,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    files: Vec<ModrinthFile>,
    dependencies: Vec<ModrinthDependency>,
}

#[derive(Debug, Deserialize)]
struct ModrinthFile {
    hashes: Option<ModrinthHashes>,
    url: String,
    filename: String,
    primary: Option<bool>,
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ModrinthHashes {
    sha512: Option<String>,
    sha1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthDependency {
    project_id: Option<String>,
    version_id: Option<String>,
    dependency_type: Option<String>,
}

fn parse_side(s: Option<&str>) -> Side {
    match s.unwrap_or("both").to_lowercase().as_str() {
        "required" | "both" => Side::Both,
        "optional" | "unsupported" => Side::Both,
        "client" => Side::Client,
        "server" => Side::Server,
        _ => Side::Both,
    }
}

fn parse_release_type(s: &str) -> ReleaseType {
    match s {
        "release" => ReleaseType::Release,
        "beta" => ReleaseType::Beta,
        "alpha" => ReleaseType::Alpha,
        _ => ReleaseType::Release,
    }
}

fn parse_loader(s: &str) -> Option<LoaderKind> {
    match s {
        "fabric" => Some(LoaderKind::Fabric),
        "quilt" => Some(LoaderKind::Quilt),
        "forge" => Some(LoaderKind::Forge),
        "neoforge" => Some(LoaderKind::NeoForge),
        _ => None,
    }
}

fn parse_dependency_type(s: Option<&str>) -> DependencyKind {
    match s.unwrap_or("optional") {
        "required" => DependencyKind::Required,
        "optional" => DependencyKind::Optional,
        "incompatible" => DependencyKind::Incompatible,
        _ => DependencyKind::Optional,
    }
}

fn convert_version(v: ModrinthVersion) -> ModVersion {
    ModVersion {
        id: v.id,
        project_id: v.project_id,
        name: v.name,
        version_number: v.version_number,
        version_type: parse_release_type(&v.version_type),
        game_versions: v.game_versions,
        loaders: v.loaders.iter().filter_map(|l| parse_loader(l)).collect(),
        files: v.files.into_iter().map(convert_file).collect(),
        dependencies: v.dependencies.into_iter().map(convert_dep).collect(),
    }
}

fn convert_file(f: ModrinthFile) -> FileArtifact {
    let hashes = f
        .hashes
        .map(|h| Hashes {
            sha512: h.sha512,
            sha1: h.sha1,
        })
        .unwrap_or_default();
    FileArtifact {
        filename: f.filename,
        url: f.url,
        hashes,
        size: f.size,
        primary: f.primary.unwrap_or(false),
    }
}

fn convert_dep(d: ModrinthDependency) -> Dependency {
    Dependency {
        project_id: d.project_id,
        version_id: d.version_id,
        kind: parse_dependency_type(d.dependency_type.as_deref()),
    }
}

fn url_encode(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

#[async_trait]
impl Registry for ModrinthClient {
    async fn search_projects(&self, query: &str) -> Result<Vec<Project>, McpmError> {
        let url = format!(
            "{}/search?query={}&limit=10&facets=[[%22project_type:mod%22]]",
            self.base_url,
            url_encode(query)
        );
        let resp =
            self.client.get(&url).send().await.map_err(|e| {
                McpmError::Registry(format!("Modrinth search request failed: {}", e))
            })?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(McpmError::Registry(format!(
                "Modrinth search returned status {}",
                status
            )));
        }
        let search: SearchResponse = resp.json().await.map_err(|e| {
            McpmError::Registry(format!("Failed to parse Modrinth search response: {}", e))
        })?;
        Ok(search
            .hits
            .into_iter()
            .map(|h| Project {
                id: h.project_id,
                slug: h.slug,
                title: h.title,
                description: h.description,
                source: SourceKind::Modrinth,
                client_side: parse_side(h.client_side.as_deref()),
                server_side: parse_side(h.server_side.as_deref()),
                website_url: None,
            })
            .collect())
    }

    async fn get_project(&self, id: &str) -> Result<Project, McpmError> {
        let url = format!("{}/project/{}", self.base_url, id);
        let resp =
            self.client.get(&url).send().await.map_err(|e| {
                McpmError::Registry(format!("Modrinth project request failed: {}", e))
            })?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(McpmError::ProjectNotFound(format!(
                "Modrinth project '{}' returned status {}",
                id, status
            )));
        }
        let project: ModrinthProject = resp.json().await.map_err(|e| {
            McpmError::Registry(format!("Failed to parse Modrinth project response: {}", e))
        })?;
        Ok(Project {
            id: project.id,
            slug: project.slug,
            title: project.title,
            description: project.description,
            source: SourceKind::Modrinth,
            client_side: parse_side(project.client_side.as_deref()),
            server_side: parse_side(project.server_side.as_deref()),
            website_url: None,
        })
    }

    async fn get_versions(&self, project_id: &str) -> Result<Vec<ModVersion>, McpmError> {
        let url = format!("{}/project/{}/version", self.base_url, project_id);
        let resp =
            self.client.get(&url).send().await.map_err(|e| {
                McpmError::Registry(format!("Modrinth versions request failed: {}", e))
            })?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(McpmError::Registry(format!(
                "Modrinth versions for '{}' returned status {}",
                project_id, status
            )));
        }
        let versions: Vec<ModrinthVersion> = resp.json().await.map_err(|e| {
            McpmError::Registry(format!("Failed to parse Modrinth versions response: {}", e))
        })?;
        Ok(versions.into_iter().map(convert_version).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_side() {
        assert_eq!(parse_side(Some("required")), Side::Both);
        assert_eq!(parse_side(Some("optional")), Side::Both);
        assert_eq!(parse_side(Some("client")), Side::Client);
        assert_eq!(parse_side(Some("server")), Side::Server);
        assert_eq!(parse_side(None), Side::Both);
    }

    #[test]
    fn test_parse_release_type() {
        assert_eq!(parse_release_type("release"), ReleaseType::Release);
        assert_eq!(parse_release_type("beta"), ReleaseType::Beta);
        assert_eq!(parse_release_type("alpha"), ReleaseType::Alpha);
    }

    #[test]
    fn test_parse_loader() {
        assert_eq!(parse_loader("fabric"), Some(LoaderKind::Fabric));
        assert_eq!(parse_loader("quilt"), Some(LoaderKind::Quilt));
        assert_eq!(parse_loader("forge"), Some(LoaderKind::Forge));
        assert_eq!(parse_loader("neoforge"), Some(LoaderKind::NeoForge));
        assert_eq!(parse_loader("unknown"), None);
    }

    #[test]
    fn test_url_encode() {
        assert_eq!(url_encode("sodium"), "sodium");
        assert_eq!(url_encode("create plus"), "create%20plus");
    }

    #[test]
    fn test_extract_xml_versions() {
        let xml = r#"<metadata><versioning><versions>
            <version>21.0.167</version>
            <version>21.1.0</version>
        </versions></versioning></metadata>"#;
        assert_eq!(
            extract_xml_versions(xml),
            vec!["21.0.167".to_string(), "21.1.0".to_string()]
        );
    }
}
