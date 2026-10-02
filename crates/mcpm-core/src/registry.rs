use crate::{LoaderKind, McpmError, ReleaseType, Side, SourceKind};
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub slug: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub source: SourceKind,
    pub client_side: Side,
    pub server_side: Side,
}

#[derive(Debug, Clone)]
pub struct ModVersion {
    pub id: String,
    pub project_id: String,
    pub name: Option<String>,
    pub version_number: String,
    pub version_type: ReleaseType,
    pub game_versions: Vec<String>,
    pub loaders: Vec<LoaderKind>,
    pub files: Vec<FileArtifact>,
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone)]
pub struct FileArtifact {
    pub filename: String,
    pub url: String,
    pub hashes: Hashes,
    pub size: Option<u64>,
    pub primary: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Hashes {
    pub sha512: Option<String>,
    pub sha1: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Dependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub kind: DependencyKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    Required,
    Optional,
    Incompatible,
}

#[async_trait]
pub trait Registry: Send + Sync {
    async fn search_projects(&self, query: &str) -> Result<Vec<Project>, McpmError>;
    async fn get_project(&self, id: &str) -> Result<Project, McpmError>;
    async fn get_versions(&self, project_id: &str) -> Result<Vec<ModVersion>, McpmError>;
}

pub fn version_matches_environment(
    version: &ModVersion,
    game_version: &str,
    loader: LoaderKind,
) -> bool {
    version.game_versions.contains(&game_version.to_string()) && version.loaders.contains(&loader)
}

pub fn version_matches_preferences(version: &ModVersion, prefs: &crate::Preferences) -> bool {
    let base_ok = match prefs.release_type {
        ReleaseType::Release => version.version_type == ReleaseType::Release,
        ReleaseType::Beta => matches!(
            version.version_type,
            ReleaseType::Release | ReleaseType::Beta
        ),
        ReleaseType::Alpha => true,
    };

    if base_ok {
        return true;
    }

    prefs.allow_beta
        && matches!(
            version.version_type,
            ReleaseType::Release | ReleaseType::Beta
        )
}

pub fn select_primary_file(files: &[FileArtifact]) -> Option<&FileArtifact> {
    files
        .iter()
        .find(|f| f.primary)
        .or_else(|| files.iter().find(|f| f.filename.ends_with(".jar")))
}

pub fn infer_mod_side(client_side: &Side, server_side: &Side) -> Side {
    match (client_side, server_side) {
        (Side::Both, Side::Both) => Side::Both,
        (Side::Both, Side::Client) => Side::Client,
        (Side::Both, Side::Server) => Side::Server,
        (Side::Client, Side::Both) => Side::Client,
        (Side::Client, Side::Client) => Side::Client,
        (Side::Client, Side::Server) => Side::Both,
        (Side::Server, Side::Both) => Side::Server,
        (Side::Server, Side::Client) => Side::Both,
        (Side::Server, Side::Server) => Side::Server,
    }
}

#[derive(Debug, Clone)]
pub struct ResolvePlan {
    pub mods: Vec<ResolvedMod>,
    pub warnings: Vec<ResolveWarning>,
    pub conflicts: Vec<ResolveConflict>,
    pub missing: Vec<MissingRequested>,
    pub workarounds: Vec<String>,
    pub success: bool,
}

#[derive(Debug, Clone)]
pub struct MissingRequested {
    pub name: String,
    pub project_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedMod {
    pub name: String,
    pub source: SourceKind,
    pub project_id: String,
    pub slug: Option<String>,
    pub version_id: String,
    pub version_number: String,
    pub version_type: ReleaseType,
    pub filename: String,
    pub url: String,
    pub sha512: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub side: Side,
    pub reason: crate::lockfile::LockReason,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolveWarning {
    pub project_id: String,
    pub name: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ResolveConflict {
    pub project_id: String,
    pub name: String,
    pub conflicting_project_id: String,
    pub conflicting_name: String,
    pub message: String,
}
