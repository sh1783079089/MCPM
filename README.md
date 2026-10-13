# mcpm

`mcpm` 是一个计划中的 Minecraft 模组包管理器，目标用户是整合包作者、服务器服主和高级玩家。

项目目标不是替代启动器，而是专注于可复现的整合包管理、依赖解析、环境选择、兼容性检查和更新计划。

## 核心定位

`mcpm` 应该像 Minecraft 整合包领域的包管理器：

- 使用 `mcpm.toml` 声明整合包意图。
- 使用 `mcpm.lock.json` 保证可复现安装。
- 抽象 Modrinth、CurseForge 和本地索引等来源。
- 解析 required、optional、incompatible、embedded 等依赖关系。
- 在 Minecraft 版本、加载器类型、加载器版本之间进行环境求解。
- 整合官方元数据、本地整合包规则和经过整理的社区兼容性规则。
- 使用安全的更新计划，避免静默改变环境。

## 早期非目标

- 微软账号登录。
- 启动游戏。
- 完整替代启动器。
- GUI 优先工作流。
- 在第一个里程碑完整支持 CurseForge。
- 自动解决所有可能的模组冲突。

## 下载与安装

CI 会在每次推送到 `main` 后构建三平台产物（在该次 Actions 运行页面的 Artifacts 下载）；推送 `v*` 标签时会发布到 GitHub Releases，并附带 `checksums.txt`（SHA-256）。

| 平台 | 产物 | 使用方式 |
|---|---|---|
| Windows x64 | `mcpm-<version>-windows-x86_64.zip` | 解压后在终端运行 `mcpm.exe`（绿色版） |
| Linux x64 | `mcpm-<version>-linux-x86_64.AppImage` | `chmod +x` 后在终端运行，CLI 参数原样透传 |
| Linux (deb) | `mcpm_<version>_amd64.deb` | `sudo apt install ./mcpm_<version>_amd64.deb`，安装到 `/usr/bin/mcpm` |
| Linux (rpm) | `mcpm-<version>-*.rpm` | `sudo rpm -ivh mcpm-<version>-*.rpm`（或 `dnf install`） |
| macOS ARM64 | `mcpm-<version>-macos-aarch64.tar.gz` | 解压后在终端运行 `mcpm`（绿色版） |

注意事项：

- AppImage 可在缺少 FUSE 的环境使用 `APPIMAGE_EXTRACT_AND_RUN=1 ./mcpm-*.AppImage ...` 运行。
- Linux 包基于 Ubuntu 24.04 构建，需要较新的 glibc；`deb` 声明 `libssl3 | libssl3t64` 兼容不同发行版命名。
- Windows 安装器（Inno Setup，自动加入 PATH）与 WinGet 分发计划在正式版提供。

## 文档索引

- [01. 架构设计](doc/01-architecture.md)
- [02. 配置与锁文件](doc/02-configuration-and-lockfile.md)
- [03. 解析器设计](doc/03-resolver.md)
- [04. 兼容性与玩家反馈](doc/04-compatibility-feedback.md)
- [05. CLI 设计](doc/05-cli.md)
- [06. Rust 实现方案](doc/06-rust-implementation.md)
- [07. 开发路线](doc/07-roadmap.md)

## 推荐第一里程碑

第一个可用版本应该刻意保持小范围：

- Rust CLI。
- 只支持 Modrinth。
- 只支持 Fabric。
- 用户手动指定 Minecraft 版本和加载器。
- 支持 `mcpm.toml` 和 `mcpm.lock.json`。
- 支持 required 依赖解析。
- 支持文件下载。
- 支持 SHA-512 校验。
- 支持安装到 `mods/`。

目标工作流示例：

```bash
mcpm init --minecraft 1.20.1 --loader fabric
mcpm add sodium
mcpm add iris
mcpm install
mcpm check
```
