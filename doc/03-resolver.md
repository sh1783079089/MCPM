# 03. 解析器设计

## 职责

Resolver 负责决定应该安装什么。它不能下载文件，也不能修改项目目录。

输入：

- 用户请求的模组。
- 环境约束或已锁定环境。
- Registry 元数据。
- 用户偏好。
- 官方依赖和冲突。
- 本地兼容性规则。
- 社区兼容性规则。

输出：

- 选中的环境。
- 选中的模组版本。
- 安装、更新、删除动作。
- 冲突。
- 警告。
- 评分和解释。

## 解析模式

### 锁定环境解析

用于 Minecraft 版本和加载器已经确定的情况。

```text
已知：
  Minecraft 1.20.1
  Fabric
  用户请求的模组

求解：
  兼容的模组版本和依赖
```

### 探索环境解析

用于作者还没有选择 Minecraft 版本或加载器的情况。

```text
已知：
  用户请求的模组
  Minecraft 版本范围
  候选加载器

求解：
  排序后的候选环境
```

## 固定环境算法

```text
1. 用用户请求的模组初始化队列。
2. 在 registry 中定位项目。
3. 获取项目所有版本。
4. 按 Minecraft 版本和加载器过滤。
5. 按发布类型偏好过滤。
6. 选择最佳兼容版本。
7. 将 required 依赖加入队列。
8. 将 optional 依赖记录为警告。
9. 将 incompatible 依赖记录为冲突。
10. 对已选项目去重。
11. 应用兼容性规则。
12. 返回解析计划。
```

伪代码：

```text
while queue is not empty:
    request = queue.pop()
    project = locate_project(request)
    versions = registry.get_versions(project)
    version = select_best_version(versions, environment, preferences)

    if project already selected:
        ensure selected version satisfies current request
        continue

    selected.add(project, version)

    for dependency in version.dependencies:
        if dependency.kind == required:
            queue.push(dependency)
        if dependency.kind == optional:
            warnings.add(dependency)
        if dependency.kind == incompatible:
            conflicts.add(dependency)

apply_compatibility_rules(selected, environment)
return plan
```

## 环境求解

环境求解把 Minecraft 版本、加载器类型，以及后续的加载器版本都视为变量。

```text
1. 生成候选环境。
2. 对每个候选环境运行固定环境解析。
3. 统计可用的目标模组数量。
4. 统计缺失的目标模组。
5. 检查依赖闭包。
6. 应用兼容性风险规则。
7. 为每个候选环境评分。
8. 返回排序结果。
```

候选环境：

```text
Minecraft version
Loader kind
Loader version，早期可选
Side
```

初始候选版本可以先使用人工维护的主流列表：

```text
1.21.1
1.20.1
1.19.2
1.18.2
1.16.5
1.12.2
```

后续再从 Mojang version manifest 和 registry 元数据中动态生成。

## 评分模型

示例评分：

```text
+50 所有用户请求的模组都可用
+10 required 依赖全部解析成功
+10 优先 release 版本
+10 命中用户加载器偏好
+10 Minecraft 版本较新或较主流
+10 无高置信社区冲突
-30 每个缺失的目标模组
-25 hard conflict
-15 在不偏好的情况下必须使用 beta 或 alpha
-10 必须使用过旧加载器版本
-10 需要 workaround
-5 每个 soft warning
```

评分只是决策辅助，不是真理。输出必须解释原因。

## 更新计划

默认更新模式应该是安全更新。

安全更新约束：

- 不改变 Minecraft 版本。
- 不改变加载器类型。
- 不改变 side。
- 不引入高置信 hard conflict。
- 不使用 beta 或 alpha，除非配置允许。

命令：

```bash
mcpm update
mcpm update --safe
mcpm update --allow-loader
mcpm upgrade-env --target-minecraft 1.21.1
mcpm migrate-loader --to neoforge
```

`update --allow-loader` 可以更新加载器版本，但不能改变加载器类型。

Minecraft 版本升级和加载器类型迁移必须使用显式命令。

## 加载器版本约束

加载器类型和加载器版本是两个概念。

示例：

```text
loader type: fabric
loader version: 0.15.11
```

早期实现可以不做严格的加载器版本求解。后续版本应解析 jar 元数据：

- `fabric.mod.json`
- `quilt.mod.json`
- `META-INF/mods.toml`
- `META-INF/neoforge.mods.toml`

加载器版本解析流程：

```text
1. 收集所有加载器版本约束。
2. 计算约束交集。
3. 在交集中选择最高稳定版本。
4. 如果不存在合法版本，产生冲突。
```
