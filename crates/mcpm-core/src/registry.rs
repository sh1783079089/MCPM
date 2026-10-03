use crate::{LoaderKind, McpmError, ReleaseType, Side, SourceKind, VersionRange};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub slug: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub source: SourceKind,
    pub client_side: Side,
    pub server_side: Side,
    pub website_url: Option<String>,
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
    async fn get_project(&self, source: SourceKind, id: &str) -> Result<Project, McpmError>;
    async fn get_versions(
        &self,
        source: SourceKind,
        project_id: &str,
    ) -> Result<Vec<ModVersion>, McpmError>;
}

#[derive(Debug, Clone)]
pub struct LoaderVersion {
    pub version: String,
    pub stable: bool,
}

/// Source of available loader versions for a given loader type.
#[async_trait]
pub trait LoaderVersionRegistry: Send + Sync {
    async fn get_loader_versions(
        &self,
        loader: LoaderKind,
        minecraft: &str,
    ) -> Result<Vec<LoaderVersion>, McpmError>;
}

/// Selects the highest stable loader version satisfying every constraint.
///
/// Falls back to pre-release versions only when no stable version
/// satisfies the constraints.
pub fn select_loader_version(
    candidates: &[LoaderVersion],
    constraints: &[VersionRange],
) -> Option<LoaderVersion> {
    let matches_all = |candidate: &&LoaderVersion| {
        constraints
            .iter()
            .all(|range| range.matches(&candidate.version))
    };

    let pick_highest = |stable_only: bool| -> Option<LoaderVersion> {
        candidates
            .iter()
            .filter(matches_all)
            .filter(|c| !stable_only || c.stable)
            .max_by(|a, b| crate::version::compare_versions(&a.version, &b.version))
            .cloned()
    };

    pick_highest(true).or_else(|| pick_highest(false))
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingRequested {
    pub name: String,
    pub project_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveWarning {
    pub project_id: String,
    pub name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveConflict {
    pub project_id: String,
    pub name: String,
    pub conflicting_project_id: String,
    pub conflicting_name: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VersionRange;

    fn lv(version: &str, stable: bool) -> LoaderVersion {
        LoaderVersion {
            version: version.to_string(),
            stable,
        }
    }

    #[test]
    fn test_select_loader_version_highest_stable() {
        let candidates = vec![lv("0.14.0", true), lv("0.15.11", true), lv("0.16.0", false)];
        let selected = select_loader_version(&candidates, &[]).unwrap();
        assert_eq!(selected.version, "0.15.11");
    }

    #[test]
    fn test_select_loader_version_intersection() {
        let candidates = vec![lv("0.13.0", true), lv("0.15.11", true), lv("0.16.0", true)];
        let constraints = vec![
            VersionRange::parse(">=0.15").unwrap(),
            VersionRange::parse("<0.16").unwrap(),
        ];
        let selected = select_loader_version(&candidates, &constraints).unwrap();
        assert_eq!(selected.version, "0.15.11");
    }

    #[test]
    fn test_select_loader_version_falls_back_to_prerelease() {
        let candidates = vec![lv("0.16.0", false)];
        let constraints = vec![VersionRange::parse(">=0.15").unwrap()];
        let selected = select_loader_version(&candidates, &constraints).unwrap();
        assert_eq!(selected.version, "0.16.0");
    }

    #[test]
    fn test_select_loader_version_no_match() {
        let candidates = vec![lv("0.13.0", true)];
        let constraints = vec![VersionRange::parse(">=0.15").unwrap()];
        assert!(select_loader_version(&candidates, &constraints).is_none());
    }
}
