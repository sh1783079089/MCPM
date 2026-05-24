# 02. 配置与锁文件

## 设计原则

`mcpm.toml` 表达用户意图。`mcpm.lock.json` 记录具体解析结果。

这个拆分允许整合包作者用高层目标描述整合包，同时保证安装结果可复现。

## `mcpm.toml`

配置文件由用户编辑。

### 探索态示例

适用于作者还没有决定具体 Minecraft 版本或加载器的阶段。

```toml
[pack]
name = "tech-adventure"
author = "example"
description = "A technology and adventure modpack"

[environment]
minecraft = ">=1.20 <1.22"
loader = ["fabric", "forge", "neoforge"]
side = "client"

[preferences]
release_type = "release"
allow_beta = false
environment_strategy = "most_compatible"
source = ["modrinth", "curseforge"]

[mods]
create = "latest"
sodium = "latest"
jei = "latest"
farmers-delight = "latest"
```

### 锁定环境示例

适用于作者已经选择了环境的阶段。

```toml
[pack]
name = "tech-adventure"

[environment]
minecraft_range = ">=1.20 <1.22"       # 保留原始范围，供 unlock-env 还原
minecraft = "1.20.1"
loader_candidates = ["fabric", "forge", "neoforge"]  # 保留原始候选列表
loader = "forge"
loader_version = "47.2.20"
side = "client"

[preferences]
release_type = "release"
allow_beta = false
source = ["modrinth", "curseforge"]

[mods]
create = "latest"
jei = "latest"
farmers-delight = "latest"
embeddium = "latest"
```

`minecraft_range` 和 `loader_candidates` 是锁定态保留的原始探索范围信息，`unlock-env` 时用于还原探索态。它们在探索态中不存在。

### 模式推断规则

配置模式不通过显式字段声明，而是由 `[environment]` 字段值的类型推断：

| 字段 | 探索态 | 锁定态 |
|------|--------|--------|
| `minecraft` | 范围表达式（如 `">=1.20 <1.22"`） | 精确版本（如 `"1.20.1"`） |
| `minecraft_range` | 不存在 | 保留原始范围 |
| `loader` | 数组（如 `["fabric", "forge"]`） | 字符串（如 `"forge"`） |
| `loader_candidates` | 不存在 | 保留原始候选列表 |
| `loader_version` | 不存在 | 精确版本（如 `"47.2.20"`） |
| `environment_strategy` | 存在（在 `[preferences]`） | 不存在 |

推断逻辑：
- `minecraft` 是范围表达式 → 探索态；是精确版本 → 锁定态
- `loader` 是数组 → 探索态；是字符串 → 锁定态
- 锁定态下，`minecraft_range` / `loader_candidates` 记录原始探索范围，`unlock-env` 时复原
- 锁定态下 `loader_version` 记录已确定的加载器版本
- `environment_strategy` 仅在探索态存在，锁定后移除

### 模组条目格式

简单格式：

```toml
[mods]
sodium = "latest"
```

展开格式：

```toml
[mods]
sodium = { version = "latest", source = "modrinth", side = "client" }
```

固定项目身份：

```toml
[mods]
sodium = { source = "modrinth", project_id = "AANobbMI", version = "latest" }
```

实现时不能假设不同来源中同名项目就是同一个项目。

## 本地兼容性规则

整合包作者可以编码本地已知冲突。

```toml
[[compatibility.conflicts]]
mods = [
  { id = "mod_a", version = ">=1.4 <1.5" },
  { id = "mod_b", version = ">=3.2 <3.3" }
]
minecraft = "1.20.1"
loader = "fabric"
severity = "hard"
reason = "Crashes on startup in this pack"
```

Workaround 示例：

```toml
[[compatibility.workarounds]]
mods = ["mod_a", "mod_b"]
reason = "Disable feature X in mod_a"

[[compatibility.workarounds.actions]]
type = "config_change"
file = "config/mod_a.toml"
key = "features.x"
value = false
```

本地规则可以用于技术冲突，也可以用于玩法设计冲突。

## `mcpm.lock.json`

锁文件由机器生成，应该作为安装的事实来源。

```json
{
  "lockfile_version": 1,
  "mcpm_version": "0.1.0",
  "environment": {
    "minecraft": "1.20.1",
    "loader": "forge",
    "loader_version": "47.2.20",
    "side": "client"
  },
  "ruleset": {
    "source": "mcpm-community",
    "version": "2026.05.24",
    "hash": "sha256:..."
  },
  "mods": [
    {
      "name": "Create",
      "source": "modrinth",
      "project_id": "LNytGWDc",
      "version_id": "abc123",
      "version_number": "0.5.1.f",
      "filename": "create-1.20.1-0.5.1.f.jar",
      "url": "https://cdn.modrinth.com/...",
      "sha512": "...",
      "size": 12345678,
      "side": "both",
      "reason": "requested",
      "dependencies": []
    }
  ]
}
```

## 锁文件规则

- `mcpm install` 使用锁文件，不重新解析版本。
- `mcpm update` 在修改锁文件之前生成计划。
- `mcpm upgrade-env` 在修改 Minecraft 或加载器之前生成计划。
- 当社区规则影响解析结果时，应在锁文件中记录规则集版本。
- 每个可下载文件都必须记录 hash。

## 受管理文件状态

安装器应在 `.mcpm/state.json` 记录它管理的文件。

```json
{
  "managed_files": [
    "mods/sodium-fabric-0.5.11.jar",
    "mods/iris-1.7.2.jar"
  ]
}
```

`mods/` 中非 mcpm 管理的 jar 应该提示，但默认不删除。
