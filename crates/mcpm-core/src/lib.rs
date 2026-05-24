use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LoaderKind {
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Client,
    Server,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReleaseType {
    Release,
    Beta,
    Alpha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceKind {
    Modrinth,
    CurseForge,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectRef {
    pub source: SourceKind,
    pub project_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum McpmError {
    #[error("no compatible version found for {project}")]
    NoCompatibleVersion { project: String },

    #[error("dependency conflict between {a} and {b}")]
    DependencyConflict { a: String, b: String },

    #[error("environment is not locked")]
    EnvironmentNotLocked,

    #[error("{0}")]
    Other(String),
}
