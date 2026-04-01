# Grapha

[English](../README.md)

极速代码智能，为 LLM 智能体和开发者工具而生。

Grapha 将源代码转换为标准化、图结构的表示，具备编译器级别的精度。对于 Swift，它通过二进制 FFI 桥接读取 Xcode 预编译的 Index Store，获取完整类型解析的符号图；在无编译产物时会依次回退到 SwiftSyntax 和 tree-sitter，实现即时解析。生成的图支持持久化、增量搜索/索引同步、数据流追踪、语义 effect 图和影响分析 —— 让智能体和开发者能够结构化地访问大规模代码库。

> **1,991 个 Swift 文件 — 13.1 万节点，78.7 万边 — 9.7 秒完成索引。**

## 性能

在生产级 iOS 应用上实测（1,991 个 Swift 文件，约 30 万行代码）：

| 阶段 | 耗时 |
|------|------|
| 提取（Index Store + tree-sitter 增强） | **3.6 秒** |
| 合并（模块感知的跨文件解析） | 0.3 秒 |
| 分类（入口点 + 终端操作） | 1.5 秒 |
| SQLite 持久化（延迟索引，91.8 万行） | 3.1 秒 |
| 搜索索引（BM25 via tantivy，7 个字段） | 0.7 秒 |
| **合计** | **9.7 秒** |

| 指标 | 数值 |
|------|------|
| 节点数（含源码片段） | 131,242 |
| 边数（编译器解析） | 787,021 |
| 入口点（自动检测） | 2,985 |
| 终端操作 | 11,148 |

**为什么这么快：**
- **Index Store 路径是零解析二进制 FFI** — 桥接层返回紧凑结构体 + 去重字符串表，Rust 端通过指针运算直接读取，无需 serde。
- **无锁并行提取** — 每个 rayon 线程通过 C 回调指针获得独立的提取上下文，无全局互斥锁。
- **单次 tree-sitter 解析** — 文档注释、SwiftUI 结构和本地化增强共享同一次解析。
- **基于标记的跳过** — 不含 SwiftUI/本地化标记的文件直接跳过昂贵的增强处理（字节级扫描，非 AST）。
- **延迟索引构建** — SQLite 主键和索引在批量插入完成后创建，而非逐行维护。
- **USR 作用域边解析** — 读取边通过 USR 类型前缀匹配解析，消除误报，无需后处理。

使用 `grapha index --timing` 查看逐阶段耗时明细，包括线程累计提取时间。

## 功能特性

- **编译器级精度** — 读取 Xcode 预编译的 Index Store，获取 100% 类型解析的调用图（Swift）。无编译产物时依次回退到 SwiftSyntax 和 tree-sitter。
- **增量索引** — SQLite 存储和 Tantivy 搜索默认增量同步。使用 `grapha index --full-rebuild` 强制完整重建。
- **高级搜索** — BM25 全文搜索，支持过滤器（`--kind`、`--module`、`--role`、`--fuzzy`）和上下文模式（`--context`），内联源码片段和关系信息。
- **源码片段** — 每个符号存储截断的源码片段（最多 600 字符），智能体无需读取完整文件即可浏览代码。
- **数据流追踪** — 从入口点正向追踪到终端操作（网络、持久化、缓存），或从任意符号反向追踪到受影响的入口点。
- **语义数据流图** — 通过 `grapha flow graph` 从符号导出去重后的 effect 图，包含 read、write、publish、subscribe 和终端副作用。
- **影响分析** — BFS 爆炸半径："如果我改了这个函数，什么会受影响？"
- **输出自定义** — `--fields` 标志控制显示列（file、id、module、span、snippet、visibility、signature、role），支持在 `grapha.toml` 中配置默认值。
- **跨模块分析** — 通过 `[[external]]` 配置包含本地外部仓库，实现跨仓库边解析和影响分析。
- **文件地图** — `grapha repo map` 展示每个模块的目录级符号数量概览，方便项目导航。
- **MCP 服务器** — `grapha serve --mcp` 通过 JSON-RPC stdio 暴露 6 个工具，用于 AI 智能体集成（搜索、上下文、影响、追踪、文件地图、索引）。
- **入口点检测** — 自动识别 SwiftUI View、`@Observable` 类、`fn main()`、`#[test]` 函数。
- **终端分类** — 识别网络调用、持久化（GRDB、CoreData）、缓存（Kingfisher）、统计分析等。支持通过 `grapha.toml` 扩展。
- **溯源感知的变更检测** — 边携带源码位置信息，`grapha repo changes` 可以在声明 span 不变的情况下归因方法体编辑。
- **Web UI** — 内嵌交互式图可视化（`grapha serve`）。
- **Nodus 包** — `nodus add wenext/grapha --adapter claude` 一键安装 skills、rules 和 commands，实现 grapha 感知的 AI 工作流。
- **多语言** — 目前支持 Rust 和 Swift。架构可扩展至 Java、Kotlin、C#、TypeScript。

## 安装

```bash
cargo install --path grapha
```

## 快速开始

```bash
# 索引项目
grapha index .

# 带过滤器搜索
grapha symbol search "ViewModel" --kind struct
grapha symbol search "send" --kind function --module LamaLudo --context
grapha symbol search "config" --fuzzy

# 获取符号的 360° 上下文
grapha symbol context sendMessage --format tree

# 影响分析：改了这个函数，什么会受影响？
grapha symbol impact bootstrapGame --depth 5 --format tree

# 正向追踪：入口点 → 终端操作
grapha flow trace bootstrapGame --format tree

# 反向追踪：哪些入口点会经过这个符号？
grapha flow trace handleSendResult --direction reverse --format tree

# 语义 effect 图
grapha flow graph bootstrapGame --format tree

# 列出自动检测到的入口点
grapha flow entries

# 项目导航
grapha repo map --module LamaLudo

# 仓库变更分析
grapha repo changes

# 交互式 Web UI
grapha serve --port 8765

# MCP 服务器（AI 智能体集成）
grapha serve --mcp
```

## 命令

### `grapha index` — 构建图

```bash
grapha index .                         # 索引项目（SQLite，增量）
grapha index . --full-rebuild          # 强制完整重建
grapha index . --timing                # 显示逐阶段耗时明细
grapha index . --format json           # JSON 输出（调试用）
grapha index . --store-dir /tmp/idx    # 自定义存储位置
```

自动从 DerivedData 发现 Xcode 的 Index Store，获取编译器解析的符号。无 Index Store 时自动回退到 SwiftSyntax 和 tree-sitter。SQLite 存储和搜索索引默认增量同步。

### `grapha analyze` — 一次性提取

```bash
grapha analyze src/                    # 分析目录
grapha analyze src/main.rs             # 分析单文件
grapha analyze src/ --compact          # LLM 优化的分组输出
grapha analyze src/ --filter fn,struct # 按符号类型过滤
grapha analyze src/ -o graph.json      # 输出到文件
```

### `grapha symbol search` — 全文搜索

```bash
grapha symbol search "ViewModel"                        # 基础 BM25 搜索
grapha symbol search "send" --kind function             # 按类型过滤
grapha symbol search "Config" --module FrameUI           # 按模块过滤
grapha symbol search "view" --role entry_point           # 按角色过滤
grapha symbol search "VeiwModel" --fuzzy                 # 容错拼写
grapha symbol search "sendGift" --context                # 内联源码片段 + 依赖
grapha symbol search "handle" --kind function --limit 5  # 组合使用
```

### `grapha symbol context` — 360° 符号视图

```bash
grapha symbol context Config                             # 调用者、被调用者、读取、实现
grapha symbol context bootstrapGame --format tree        # 树形输出
grapha symbol context sendGift --fields module,signature # 自定义字段
```

### `grapha symbol impact` — 影响范围分析

```bash
grapha symbol impact bootstrapGame                       # 谁依赖这个符号？
grapha symbol impact bootstrapGame --depth 5             # 更深层遍历
grapha symbol impact bootstrapGame --format tree
```

### `grapha flow trace` — 正向/反向数据流追踪

```bash
grapha flow trace bootstrapGame                          # 入口点 → 终端操作
grapha flow trace sendMessage --depth 10
grapha flow trace handleSendResult --direction reverse   # 哪些入口点会经过这里？
grapha flow trace bootstrapGame --format tree
```

### `grapha flow graph` — 语义 effect 图

```bash
grapha flow graph bootstrapGame
grapha flow graph sendMessage --depth 10 --format tree
```

### `grapha flow entries` — 列出入口点

```bash
grapha flow entries
grapha flow entries --format tree
```

### `grapha repo map` — 文件/符号概览

```bash
grapha repo map                        # 完整项目
grapha repo map --module FrameUI       # 单个模块
```

### `grapha repo changes` — Git 变更检测

```bash
grapha repo changes                    # 所有未提交的变更
grapha repo changes staged             # 仅暂存区
grapha repo changes main               # 与某个分支对比
```

### `grapha serve` — Web UI 和 MCP 服务器

```bash
grapha serve --port 8765               # Web UI，访问 http://localhost:8765
grapha serve --mcp                     # MCP 服务器（stdio）
```

### `grapha l10n` — 本地化

```bash
grapha l10n symbol body                                  # 解析本地化记录
grapha l10n usages account_forget_password --table Localizable
```

`--color auto|always|never` 控制树形输出的 ANSI 样式。`--fields` 控制输出中显示的列（详见下方输出自定义）。

## 配置

项目根目录下可选的 `grapha.toml`：

```toml
[swift]
index_store = true              # 设为 false 跳过 Index Store，仅用 tree-sitter

[output]
default_fields = ["file", "module"]  # 所有查询输出的默认字段

[[external]]
name = "FrameUI"
path = "/path/to/local/frameui"      # 将外部仓库纳入图分析

[[external]]
name = "FrameNetwork"
path = "/path/to/local/framenetwork"

[[classifiers]]
pattern = "FirebaseFirestore.*setData"
terminal = "persistence"
direction = "write"
operation = "set"
```

### 输出自定义

`--fields` 标志控制树形/JSON 输出中显示的可选列：

```bash
grapha symbol context foo --fields module,signature   # 添加模块和签名
grapha symbol context foo --fields all                # 显示所有字段
grapha symbol context foo --fields none               # 仅名称 + 类型
```

可用字段：`file`、`id`、`module`、`span`、`snippet`、`visibility`、`signature`、`role`。

### MCP 服务器

添加到 `.mcp.json` 或 Claude Code 设置：

```json
{
  "mcpServers": {
    "grapha": {
      "command": "grapha",
      "args": ["serve", "--mcp", "--path", "."]
    }
  }
}
```

工具：`search_symbols`、`get_symbol_context`、`get_impact`、`get_file_map`、`trace`、`index_project`。

### Nodus 包

```bash
nodus add wenext/grapha --adapter claude
```

安装 skills、rules 和 slash commands（`/index`、`/search`、`/impact`），实现 grapha 感知的 AI 工作流。

## 架构

### 工作空间

```
grapha-core/     共享类型（Node、Edge、Graph、ExtractionResult）
grapha-swift/    Swift 提取：Index Store → SwiftSyntax → tree-sitter 瀑布策略
grapha/          CLI 二进制、Rust 提取器、流水线、查询引擎、Web UI、MCP 服务器
nodus/           智能体工具包（skills、rules、commands）
```

### 提取瀑布策略（Swift）

```
1. Xcode Index Store（通过 Swift 桥接的二进制 FFI）
   → 编译器解析的 USR，置信度 1.0
   → 从 DerivedData 自动发现
   → 无锁并发提取（每文件独立上下文）

2. SwiftSyntax（通过 Swift 桥接的 JSON 字符串 FFI）
   → 精确语法解析，无类型解析，置信度 0.9

3. tree-sitter-swift（内嵌）
   → 快速回退，精度有限，置信度 0.6-0.8
```

Index Store 提取后，tree-sitter 在单次共享解析中增强文档注释、SwiftUI 视图结构和本地化元数据。不含 SwiftUI/本地化标记的文件完全跳过增强处理。

### 流水线

```
发现 → 提取 → 片段 → 合并 → 分类 → 压缩 → 存储 → 查询/服务
         ↑       ↑       ↑       ↑
    Index Store / 源码   模块感知  入口点
    SwiftSyntax / 截取   解析     + 终端操作
    tree-sitter
```

### 图模型

节点代表符号（函数、类型、属性），边代表关系并附带置信度评分。

**节点类型：** `function`（函数）、`struct`（结构体）、`enum`（枚举）、`trait`（特征）、`protocol`（协议）、`extension`（扩展）、`property`（属性）、`field`（字段）、`variant`（枚举变体）、`constant`（常量）、`type_alias`（类型别名）、`impl`（实现块）、`module`（模块）、`view`（视图）、`branch`（分支）

**边类型：**

| 类型 | 含义 |
|------|------|
| `calls` | 函数/方法调用 |
| `implements` | 协议遵循 / trait 实现 |
| `inherits` | 超类 / 超 trait |
| `contains` | 结构嵌套 |
| `type_ref` | 类型引用 |
| `uses` | 导入语句 |
| `reads` / `writes` | 数据访问方向 |
| `publishes` / `subscribes` | 事件流 |

**边上的数据流注解：**

| 字段 | 用途 |
|------|------|
| `direction` | `read`、`write`、`read_write`、`pure` |
| `operation` | `fetch`、`save`、`publish`、`navigate` 等 |
| `condition` | 守卫/条件文本（条件调用时） |
| `async_boundary` | 是否跨越 async 边界 |
| `provenance` | 源文件/位置证据 |

**节点角色：**
- `entry_point` — SwiftUI View.body、@Observable 方法、fn main、#[test]
- `terminal` — 网络、持久化、缓存、事件、钥匙串、搜索

## 支持的语言

| 语言 | 提取方式 | 类型解析 |
|------|----------|----------|
| **Swift** | tree-sitter + Xcode Index Store | 编译器级（USR） |
| **Rust** | tree-sitter | 基于名称 |

按语言分 crate 的架构（`grapha-swift/`，未来 `grapha-java/` 等）支持以相同模式添加新语言：编译器索引 → 语法解析器 → tree-sitter 回退。

## 开发

```bash
cargo build                    # 构建所有工作空间 crate
cargo test                     # 运行所有测试（约 295 个）
cargo build -p grapha-core     # 仅构建共享类型
cargo build -p grapha-swift    # 构建 Swift 提取器
cargo run -p grapha -- <cmd>   # 运行 CLI
cargo clippy                   # 代码检查
cargo fmt                      # 代码格式化
```

## 许可证

MIT
