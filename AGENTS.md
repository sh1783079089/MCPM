# AGENTS.md

AI agent configuration and project guidelines for `mcpm`.

## Rules

- **禁止在未经明确授权的情况下执行 `git commit` 或 `git push`。** 任何提交或推送操作必须先获得用户许可。

## 文档索引

| 编号 | 文件 | 说明 |
|------|------|------|
| 01 | [doc/01-architecture.md](doc/01-architecture.md) | 架构设计 |
| 02 | [doc/02-configuration-and-lockfile.md](doc/02-configuration-and-lockfile.md) | 配置与锁文件 |
| 03 | [doc/03-resolver.md](doc/03-resolver.md) | 解析器设计 |
| 04 | [doc/04-compatibility-feedback.md](doc/04-compatibility-feedback.md) | 兼容性与玩家反馈 |
| 05 | [doc/05-cli.md](doc/05-cli.md) | CLI 设计 |
| 06 | [doc/06-rust-implementation.md](doc/06-rust-implementation.md) | Rust 实现方案 |
| 07 | [doc/07-roadmap.md](doc/07-roadmap.md) | 开发路线 |

## 项目概述

- **语言**: Rust
- **目标**: Minecraft 模组包管理器，专注于可复现的整合包管理、依赖解析、环境选择和兼容性检查
- **第一个里程碑**: Modrinth only / Fabric only / CLI only
