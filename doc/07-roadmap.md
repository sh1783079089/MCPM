# 07. 开发路线

## 阶段 0：项目骨架

目标：

```text
项目可以编译，并且 `mcpm --help` 可用。
```

任务：

- 创建 Rust workspace。
- 添加 `mcpm-cli` 和 `mcpm-core`。
- 添加 `clap` 命令骨架。
- 添加 tracing/logging。
- 添加基础数据模型。
- 添加 CI，执行 fmt、clippy、test 和 build。

## 阶段 1：Modrinth 与固定环境安装

目标：

```bash
mcpm init --minecraft 1.20.1 --loader fabric
mcpm add sodium
mcpm install
```

任务：

- 读取和写入 `mcpm.toml`。
- 实现 Modrinth 搜索。
- 实现 Modrinth 版本获取。
- 按 Minecraft 和加载器过滤版本。
- 生成 `mcpm.lock.json`。
- 下载文件。
- 校验 SHA-512。
- 安装到 `mods/`。

验收：

- 能从 Modrinth 安装 Sodium。
- 重复 install 不会不必要地重复下载。
- 锁文件固定精确版本和 hash。

## 阶段 2：依赖解析

目标：

```bash
mcpm add iris
mcpm install
```

任务：

- 递归解析 required 依赖。
- 对 optional 依赖给出警告。
- 检测 incompatible 依赖。
- 对已选项目去重。
- 添加 `mcpm list`。
- 添加 `mcpm check`。

验收：

- required 依赖会自动安装。
- 冲突会产生可执行的错误信息。

## 阶段 3：环境求解

目标：

```bash
mcpm add create sodium jei
mcpm solve-env
```

任务：

- 生成候选 Minecraft 和加载器组合。
- 对每个候选组合运行固定环境解析。
- 为候选环境评分。
- 显示缺失模组和警告。
- 添加 `mcpm use <candidate>`。

验收：

- 工具能基于用户请求的模组推荐候选环境。

## 阶段 4：更新计划

目标：

```bash
mcpm update
mcpm update --allow-loader
mcpm upgrade-env
```

任务：

- 对比当前锁文件和可用版本。
- 实现安全更新。
- 阻止需要 Minecraft 变化的普通更新。
- 生成加载器版本更新计划。
- 生成环境升级计划。

验收：

- 默认 update 不改变 Minecraft 版本或加载器类型。
- 环境升级必须显式、基于计划。

## 阶段 5：本地兼容性规则

目标：

```text
整合包作者可以编码本地冲突和 workaround。
```

任务：

- 从 `mcpm.toml` 解析本地兼容性规则。
- 应用 hard conflict。
- 应用 soft conflict。
- 显示 workaround 提示。
- 将规则影响纳入候选评分。

验收：

- 本地 hard conflict 可以阻止某个版本。
- 本地 soft conflict 可以降低候选评分。

## 阶段 6：Jar 元数据解析

目标：

```text
使用 jar 内部元数据做更强校验。
```

任务：

- 解析 `fabric.mod.json`。
- 解析 `quilt.mod.json`。
- 解析 `META-INF/mods.toml`。
- 解析 `META-INF/neoforge.mods.toml`。
- 提取加载器版本约束。
- 校验 side 约束。
- 检测重复 mod ID。

验收：

- 能检测 client-only 模组被放到 server。
- 能报告加载器版本要求。

## 阶段 7：社区规则集

目标：

```bash
mcpm sync-rules
mcpm check --latest-rules
```

任务：

- 下载版本化规则集。
- 校验规则集 hash。
- 缓存规则集。
- 在锁文件中记录规则集。
- 解析时应用规则。

验收：

- 社区规则可以影响解析结果，但不会静默改变旧锁文件的安装行为。

## 阶段 8：崩溃报告分析器

目标：

```bash
mcpm check-crash latest.log
```

任务：

- 从日志中提取环境。
- 提取模组列表。
- 提取异常类型。
- 构建崩溃签名。
- 匹配已知兼容性规则。
- 生成匿名观察事件草稿。

验收：

- 能识别常见 Mixin、缺依赖、重复模组和 side 错误。

## 阶段 9：CurseForge 支持

目标：

```text
支持 CurseForge 项目和 manifest。
```

任务：

- 实现 CurseForge API client。
- 支持 API key 配置。
- 处理缺失 download URL 的文件。
- 导入 CurseForge manifest。
- 导出 CurseForge manifest。

验收：

- CurseForge manifest 导入可用。
- 不可自动下载的文件有清晰的手动说明。

## 阶段 10：导入导出与服务端包

目标：

```bash
mcpm export mrpack
mcpm export server-pack
```

任务：

- 导入和导出 Modrinth `.mrpack`。
- 导出服务端包。
- 区分 client/server/both 模组。
- 包含 config、scripts 等 overrides。

验收：

- 生成的整合包可以被常见启动器工作流消费。

## 阶段 11：反向导入已有模组

目标：

```bash
mcpm import mods
```

将现有 `mods/` 目录中的模组 jar 反向解析并写入 `mcpm.toml`，方便已有整合包迁移到 mcpm 管理。

任务：

- 扫描 `mods/` 目录下所有 `.jar` 文件。
- 从 jar 中提取 `fabric.mod.json` 元数据（id、version、name、environment）。
- 用 mod ID 或名称查询 Modrinth，获取项目 slug 和 project_id。
- 合并到 `mcpm.toml` 的 `[mods]` 段：
  - 已存在的条目跳过不覆盖。
  - Modrinth 找到的标记 `source = "modrinth"`，记录 project_id 和精确版本。
  - Modrinth 找不到的标记 `source = "local"`，不管理其下载/更新。
- 添加 `mcpm import mods --dry-run` 预览模式，仅打印将要加入的条目。
- 添加 `mcpm import mods --skip-existing / --overwrite` 覆盖策略选项（默认跳过已存在）。

验收：

- 能从一个已有 `mods/` 目录生成可用的 `mcpm.toml` 模组列表。
- 跳过已存在条目的默认行为不会覆盖用户手动配置。
- 第三方/私有模组被正确标记为 local source 并给出警告。
