#[derive(Debug, thiserror::Error)]
pub enum McpmError {
    #[error("no compatible version found for {project}")]
    NoCompatibleVersion { project: String },

    #[error("dependency conflict between {a} and {b}")]
    DependencyConflict { a: String, b: String },

    #[error("environment is not locked")]
    EnvironmentNotLocked,

    #[error("project not found: {0}")]
    ProjectNotFound(String),

    #[error("search returned no results for: {0}")]
    SearchNoResults(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("lockfile error: {0}")]
    Lockfile(String),

    #[error("registry error: {0}")]
    Registry(String),

    #[error("install error: {0}")]
    Install(String),

    #[error("hash mismatch for {filename}: expected {expected}, got {actual}")]
    HashMismatch {
        filename: String,
        expected: String,
        actual: String,
    },

    #[error("{0}")]
    Other(String),
}
