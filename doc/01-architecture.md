# 01. 架构设计

## 系统总览

项目应拆成本地客户端和未来可选的远程服务。

```text
mcpm-client
  本地 CLI，以及未来可能的 GUI 集成。
  负责读取配置、解析模组、写入锁文件、下载文件和安装文件。

mcpm-registry-service
  未来可选的元数据聚合服务。
  缓存 Modrinth 和 CurseForge 数据，并维护跨来源项目映射。

mcpm-compat-service
  未来可选的兼容性服务。
  收集玩家观察事件、聚合崩溃签名，并发布版本化的兼容性规则集。
```

早期版本只需要实现 `mcpm-client`。

## 客户端 Workspace 结构

推荐使用 Rust workspace：

```text
mcpm/
├── Cargo.toml
├── crates/
│   ├── mcpm-cli/
│   ├── mcpm-core/
│   ├── mcpm-config/
│   ├── mcpm-lockfile/
│   ├── mcpm-registry/
│   ├── mcpm-resolver/
│   ├── mcpm-compat/
│   ├── mcpm-installer/
│   ├── mcpm-metadata/
│   ├── mcpm-cache/
│   └── mcpm-export/
├── tests/
│   ├── fixtures/
│   └── integration/
└── doc/
```

早期可以少建一些 crate：

```text
mcpm-cli
mcpm-core
mcpm-registry
mcpm-resolver
mcpm-installer
```

当代码自然变复杂后，再拆出 `mcpm-config`、`mcpm-lockfile`、`mcpm-compat` 等 crate。

## Crate 职责

| Crate | 职责 |
|---|---|
| `mcpm-cli` | 命令解析、人类可读输出、命令编排。 |
| `mcpm-core` | 共享数据模型、通用错误、标识符、版本类型。 |
| `mcpm-config` | 读取和写入 `mcpm.toml`。 |
| `mcpm-lockfile` | 读取和写入 `mcpm.lock.json`。 |
| `mcpm-registry` | Modrinth、CurseForge 和本地索引客户端。 |
| `mcpm-resolver` | 依赖解析、环境求解、更新计划。 |
| `mcpm-compat` | 兼容性规则、风险评分、本地和社区规则集。 |
| `mcpm-installer` | 下载、缓存、hash 校验、安装到 `mods/`。 |
| `mcpm-metadata` | 解析模组 jar 内的元数据。 |
| `mcpm-cache` | API 响应缓存、文件缓存、规则集缓存。 |
| `mcpm-export` | 导入导出 `.mrpack`、CurseForge manifest 和服务端包。 |

## 依赖边界

最重要的边界是：决策逻辑和副作用分离。

- Resolver 不访问 HTTP。
- Resolver 不下载文件。
- Resolver 不写入 `mods/`。
- Installer 不选择版本。
- Registry 客户端不修改项目文件。
- CLI 负责编排，不承载核心业务逻辑。

推荐数据流：

```text
mcpm.toml
→ config parser
→ registry queries
→ resolver
→ plan
→ lockfile
→ installer
→ mods/
```

## 主数据流

```text
1. 从 mcpm.toml 读取用户意图。
2. 查询 registry 获取项目和版本元数据。
3. 构建候选版本和依赖边。
4. 应用环境约束。
5. 应用官方依赖和冲突。
6. 应用本地和社区兼容性规则。
7. 生成解析计划。
8. 写入或更新 mcpm.lock.json。
9. 下载文件到缓存。
10. 校验 hash。
11. 将受管理文件安装到 mods/。
```

## 运行时目录模型

项目目录：

```text
pack/
├── mcpm.toml
├── mcpm.lock.json
├── mods/
├── config/
└── .mcpm/
    ├── state.json
    └── plans/
```

全局缓存：

```text
Windows: %LOCALAPPDATA%\mcpm\cache
Linux: ~/.cache/mcpm
macOS: ~/Library/Caches/mcpm
```

文件缓存应该以内容 hash 作为 key，而不是以 URL 作为 key。
