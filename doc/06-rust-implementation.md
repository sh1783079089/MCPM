# 06. Rust 实现方案

## 推荐工具链

核心依赖：

```toml
clap = "4"
tokio = "1"
reqwest = "0.12"
serde = "1"
serde_json = "1"
toml = "0.8"
semver = "1"
anyhow = "1"
thiserror = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
indicatif = "0.17"
sha2 = "0.10"
hex = "0.4"
zip = "2"
directories = "5"
url = "2"
futures = "0.3"
indexmap = "2"
time = "0.3"
async-trait = "0.1"
```

测试依赖：

```toml
insta = "1"
wiremock = "0.6"
tempfile = "3"
assert_cmd = "2"
predicates = "3"
```

## 核心类型

```rust
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
```

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileArtifact {
    pub filename: String,
    pub url: String,
    pub hashes: Hashes,
    pub size: Option<u64>,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hashes {
    pub sha512: Option<String>,
    pub sha1: Option<String>,
}
```

## Registry Trait

```rust
#[async_trait::async_trait]
pub trait Registry: Send + Sync {
    async fn search_projects(&self, query: &str) -> Result<Vec<Project>>;
    async fn get_project(&self, project: &ProjectRef) -> Result<Project>;
    async fn get_versions(&self, project: &ProjectRef) -> Result<Vec<ModVersion>>;
}
```

实现：

```text
ModrinthRegistry
CurseForgeRegistry，后续实现
LocalRegistry，后续实现
CompositeRegistry，后续实现
```

## Resolver 接口

```rust
pub enum ResolveEnvironmentMode {
    Locked(Environment),
    Explore(EnvironmentConstraints),
}

pub struct ResolveRequest {
    pub requested_mods: Vec<UserModRequest>,
    pub environment: ResolveEnvironmentMode,
    pub preferences: Preferences,
    pub compatibility_rules: Vec<CompatibilityRule>,
}
```

```rust
pub struct ResolvePlan {
    pub environment: Environment,
    pub selected_mods: Vec<ResolvedMod>,
    pub actions: Vec<PlanAction>,
    pub conflicts: Vec<Conflict>,
    pub warnings: Vec<Warning>,
    pub score: i32,
    pub success: bool,
}
```

## 错误处理

库 crate 应使用 `thiserror` 定义结构化错误。

```rust
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("no compatible version found for {project}")]
    NoCompatibleVersion { project: String },

    #[error("dependency conflict between {a} and {b}")]
    DependencyConflict { a: String, b: String },

    #[error("environment is not locked")]
    EnvironmentNotLocked,
}
```

CLI 层可以使用 `anyhow` 做顶层错误传播，并格式化成人类友好的错误信息。

## Modrinth 实现要点

需要的接口：

```text
GET /v2/search
GET /v2/project/{id|slug}
GET /v2/project/{id|slug}/version
GET /v2/version/{id}
```

版本选择需要检查：

- `game_versions` 包含目标 Minecraft 版本。
- `loaders` 包含目标加载器。
- `version_type` 符合偏好。
- 存在 primary file。
- 存在 SHA-512 或 SHA-1 hash。

## Installer 实现要点

安装流程：

```text
1. 读取锁文件。
2. 按 hash 检查文件缓存。
3. 下载缺失文件。
4. 校验 SHA-512 或 SHA-1。
5. 将 jar 放入 mods/。
6. 更新 .mcpm/state.json。
```

安装器默认不删除非 mcpm 管理的文件。

## 测试策略

关键测试：

- required 依赖会被安装。
- optional 依赖只产生警告。
- incompatible 依赖产生冲突。
- 固定环境选择最新兼容 release。
- 环境求解能正确排序候选。
- 社区 hard conflict 能阻止风险版本。
- 基于锁文件的安装不会重新解析。

Resolver 测试应使用 mock registry。核心测试不要依赖真实 Modrinth API。
