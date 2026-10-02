# 05. CLI 设计

## 设计目标

- 常见工作流要短。
- 求解和应用变更分离。
- 不静默改变 Minecraft 版本或加载器类型。
- 支持人类可读输出和 `--json` 输出。
- 错误信息必须给出可执行的下一步。

## 第一里程碑命令

```bash
mcpm init --minecraft 1.20.1 --loader fabric
mcpm add sodium
mcpm add iris
mcpm install
mcpm list
mcpm check
```

## 后续命令

```bash
mcpm solve
mcpm solve-env
mcpm use <candidate>
mcpm lock-env
mcpm un-lock-env
mcpm update
mcpm update --allow-loader
mcpm upgrade-env
mcpm apply-plan
mcpm check-crash <file>
mcpm sync-rules
mcpm import mrpack <file>
mcpm export mrpack
mcpm import curseforge <file>
mcpm export curseforge
mcpm export server-pack
```

> 实现状态：
> 已实现 `solve-env`、`use`、`lock-env`、`un-lock-env`、`update`、`update --allow-loader`、`upgrade-env`、`migrate-loader`、`apply-plan`。
> `solve`（固定环境解析计划）尚未单独暴露；`check-crash`、`sync-rules`、导入导出系列未实现。

## 探索工作流

```bash
mcpm init
mcpm add create sodium jei farmers-delight
mcpm solve-env
```

期望输出：

```text
Best environments:

1. Minecraft 1.20.1 + Forge
   Requested mods available: 4/4
   Dependencies: 23
   Known risks: 1 warning
   Score: 91

2. Minecraft 1.20.1 + NeoForge
   Requested mods available: 4/4
   Dependencies: 24
   Known risks: 2 warnings
   Score: 87

3. Minecraft 1.20.1 + Fabric
   Requested mods available: 3/4
   Missing: Create
   Suggested alternatives: Create Fabric
   Score: 72
```

应用选中的环境：

```bash
mcpm use 1
```

锁定环境：

```bash
mcpm lock-env
```

`lock-env` 将选中环境的精确值写入 `mcpm.toml` 的 `[environment]`，同时保留原始探索范围到 `minecraft_range` 和 `loader_candidates` 字段。

`lock-env` 行为：
- 若无锁文件，先运行 `solve-env`
- 若只有一个候选环境，直接锁定；多个候选时需要用 `--choose` 指定
- 将 `minecraft` 和 `loader` 替换为精确值
- 将原始范围/候选列表写入 `minecraft_range` / `loader_candidates`
- 从 `[preferences]` 移除 `environment_strategy`
- 提示用户检查不可用的模组，不做自动替换

```bash
mcpm lock-env               # 唯一候选时直接锁定
mcpm lock-env --choose 1    # 从候选列表选择并锁定
```

## 解除锁定工作流

```bash
mcpm un-lock-env
```

`un-lock-env` 用 `minecraft_range` 和 `loader_candidates` 还原 `[environment]` 为探索态，删除精确值字段和保留字段，恢复 `[preferences]` 中的 `environment_strategy`，并清除 `mcpm.lock.json`。

## 安装工作流

```bash
mcpm install
```

行为：

- 读取 `mcpm.lock.json`。
- 下载缺失文件。
- 校验 hash。
- 将受管理 jar 安装到 `mods/`。
- 不重新解析版本。
- 不改变环境。

如果没有锁文件，命令可以失败并给出指引，也可以提示先运行 `mcpm solve`。

## 更新工作流

默认：

```bash
mcpm update
```

等同于：

```bash
mcpm update --safe
```

安全更新不会改变：

- Minecraft 版本。
- 加载器类型。
- Side。

加载器版本更新：

```bash
mcpm update --allow-loader
```

环境升级：

```bash
mcpm upgrade-env --target-minecraft 1.21.1
```

加载器迁移：

```bash
mcpm migrate-loader --to neoforge
```

## 输出模式

人类输出应该简洁，并解释关键原因。

机器输出：

```bash
mcpm solve --json
mcpm check --json
```

## 错误信息风格

错误应该包含失败原因和可能的下一步。

示例：

```text
No compatible version found for Create on Minecraft 1.21.1 + Fabric.

Try:
- mcpm solve-env
- mcpm use --minecraft 1.20.1 --loader forge
```
