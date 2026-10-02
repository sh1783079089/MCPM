pub mod config;
pub mod error;
pub mod lockfile;
pub mod registry;
pub mod version;

use serde::{Deserialize, Serialize};

pub use config::{
    DetailedModEntry, Environment, LoaderSpec, ModEntry, PackConfig, PackMeta, Preferences,
};
pub use error::McpmError;
pub use lockfile::{LockEnvironment, LockReason, LockedMod, Lockfile, LOCKFILE_VERSION};
pub use registry::{
    infer_mod_side, select_primary_file, version_matches_environment, version_matches_preferences,
    Dependency, DependencyKind, FileArtifact, Hashes, ModVersion, Project, Registry,
    ResolveConflict, ResolvePlan, ResolveWarning, ResolvedMod,
};
pub use version::{compare_versions, ModVersionNumber, VersionRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl std::fmt::Display for LoaderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoaderKind::Fabric => write!(f, "fabric"),
            LoaderKind::Quilt => write!(f, "quilt"),
            LoaderKind::Forge => write!(f, "forge"),
            LoaderKind::NeoForge => write!(f, "neoforge"),
        }
    }
}

/// Returns well-known platform dependencies for the given loader.
/// These are mods that the loader ecosystem expects to be present.
pub fn platform_dependencies(loader: LoaderKind) -> Vec<(&'static str, &'static str)> {
    match loader {
        LoaderKind::Fabric => vec![("fabric-api", "P7dR8mSH")],
        _ => vec![],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Client,
    Server,
    #[default]
    Both,
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Side::Client => write!(f, "client"),
            Side::Server => write!(f, "server"),
            Side::Both => write!(f, "both"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReleaseType {
    Release,
    Beta,
    Alpha,
}

impl std::fmt::Display for ReleaseType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReleaseType::Release => write!(f, "release"),
            ReleaseType::Beta => write!(f, "beta"),
            ReleaseType::Alpha => write!(f, "alpha"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Modrinth,
    CurseForge,
    Local,
}

impl std::fmt::Display for SourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceKind::Modrinth => write!(f, "modrinth"),
            SourceKind::CurseForge => write!(f, "curseforge"),
            SourceKind::Local => write!(f, "local"),
        }
    }
}
