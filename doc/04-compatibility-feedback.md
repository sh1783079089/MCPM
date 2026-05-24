# 04. 兼容性与玩家反馈

## 问题

模组冲突不一定由 Modrinth、CurseForge 或模组作者声明。它们经常来自玩家体验：

- 崩溃报告。
- Issue tracker。
- Discord 讨论。
- 服务端启动失败。
- 视觉 bug。
- 性能回退。
- 配置相关行为。

项目应该把社区反馈视为证据，而不是直接视为事实。

## 三层兼容性模型

```text
官方元数据
  Registry 或模组元数据声明的依赖和冲突。

本地规则
  整合包作者写入的 pack-specific 规则。

社区规则
  从玩家观察事件整理、审核、版本化后发布的规则。
```

决策优先级：

```text
官方硬依赖/硬冲突
> 本地整合包规则
> 经过整理的社区规则
> 高置信观察事件
> 低置信观察事件
```

## 观察事件与规则

原始用户反馈应存为观察事件。

观察事件示例：

```json
{
  "type": "crash_report",
  "minecraft": "1.20.1",
  "loader": "fabric",
  "loader_version": "0.15.11",
  "java_version": "17.0.9",
  "side": "client",
  "mods": [
    {
      "source": "modrinth",
      "project_id": "AANobbMI",
      "version_id": "abc"
    }
  ],
  "symptom": "crash_on_startup",
  "crash_signature": "MixinApplyError: ...",
  "created_at": "2026-05-24T00:00:00Z"
}
```

经过整理的规则来自多个观察事件。

规则示例：

```json
{
  "id": "community:mod-a-mod-b-mixin-crash",
  "type": "hard_conflict",
  "severity": "high",
  "confidence": 0.91,
  "evidence_count": 74,
  "mods": [
    { "id": "mod_a", "version": ">=1.4 <1.5" },
    { "id": "mod_b", "version": ">=3.2 <3.3" }
  ],
  "environment": {
    "minecraft": "1.20.1",
    "loader": "fabric"
  },
  "resolution": [
    "Upgrade mod_a to >=1.5.2",
    "Downgrade mod_b to <3.2.0"
  ]
}
```

## 规则类型

| 类型 | 含义 | Resolver 行为 |
|---|---|---|
| `hard_conflict` | 高概率崩溃或不可用状态。 | 阻止或选择替代版本。 |
| `soft_conflict` | 明显 bug，但可能仍可运行。 | 警告并降低评分。 |
| `feature_overlap` | 功能重复，例如两个小地图。 | 提醒作者。 |
| `performance_risk` | 已知性能劣化。 | 提醒并轻微降低评分。 |
| `config_conflict` | 需要修改配置才能共存。 | 提供 workaround。 |
| `side_conflict` | 客户端/服务端放置错误。 | 警告或通过计划自动修正。 |
| `version_conflict` | 特定版本组合有问题。 | 优先选择其他版本。 |

## 置信度评分

正向信号：

- 多个报告有相同崩溃签名。
- 提供完整 lockfile。
- 来自不同用户。
- 来自不同整合包。
- 模组维护者确认。
- workaround 被验证。
- 当前版本近期仍有报告。

负向信号：

- 缺少模组版本。
- 只有一条模糊反馈。
- 同一用户重复提交。
- 报告过旧且近期无复现。
- 大量成功共存信号。
- 崩溃签名指向其他原因。

Resolver 阈值：

```text
hard conflict 且 confidence >= 0.85：默认阻止
soft conflict 且 confidence >= 0.70：警告并降低评分
performance risk：只提醒
config conflict：提示 workaround
```

## 崩溃报告分析器

未来命令：

```bash
mcpm check-crash crash-reports/crash-2026-05-24.txt
```

职责：

- 提取 Minecraft 版本。
- 提取加载器和加载器版本。
- 提取模组列表。
- 提取 Java 版本。
- 提取异常类型。
- 生成崩溃签名。
- 匹配已知兼容性规则。
- 生成匿名观察事件草稿。

常见签名：

```text
MixinApplyError
ClassNotFoundException
NoClassDefFoundError
ModLoadingException
DuplicateModException
MissingDependencyException
IncompatibleClassChangeError
```

## 隐私规则

默认上传不能包含：

- 完整日志。
- 用户名。
- 服务器 IP。
- 本地文件路径。
- 聊天内容。
- token 或凭据。

完整日志上传必须显式 opt-in，并且可预览、可脱敏。

## 规则集版本化

社区规则会影响解析结果。因此规则集必须版本化，并记录在锁文件中。

命令：

```bash
mcpm sync-rules
mcpm check --latest-rules
```

`mcpm install` 应使用锁文件中的规则集，除非用户显式更新。
