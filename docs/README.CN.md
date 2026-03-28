# Grapha

一个轻量级的结构化抽象层，将源代码转换为标准化的图结构表示，专为 LLM 消费优化。

Grapha 不依赖编译器级别的语义分析，而是通过 [tree-sitter](https://tree-sitter.github.io/) 进行快速语法解析，提取符号、关系和调用模式，并将其压缩为可遍历的节点图。这使得 AI 智能体能够以最小的上下文高效地定位、遍历和推理大规模代码库。

## 安装

```bash
cargo install --path .
```

## 使用

```bash
# 分析单个文件
grapha src/main.rs

# 分析目录（递归扫描，遵守 .gitignore）
grapha src/

# 将输出写入文件
grapha src/ -o graph.json

# 按符号类型过滤
grapha src/ --filter fn,struct,trait
```

## 输出格式

Grapha 输出一个包含 `nodes`（符号）和 `edges`（关系）的 JSON 图：

```json
{
  "version": "0.1.0",
  "nodes": [
    {
      "id": "graph.rs::Config",
      "kind": "struct",
      "name": "Config",
      "file": "graph.rs",
      "span": { "start": [10, 0], "end": [15, 1] },
      "visibility": "public",
      "metadata": {}
    }
  ],
  "edges": [
    {
      "source": "main.rs::run",
      "target": "graph.rs::Config",
      "kind": "type_ref"
    }
  ]
}
```

### 节点类型

`function`（函数）、`struct`（结构体）、`enum`（枚举）、`trait`（特征）、`impl`（实现块）、`module`（模块）、`field`（字段）、`variant`（枚举变体）

### 边类型

| 类型 | 含义 |
|------|------|
| `calls` | 函数调用另一个函数 |
| `uses` | `use` 导入语句 |
| `implements` | `impl Trait for Type` |
| `contains` | 结构嵌套（模块包含结构体，结构体包含字段） |
| `type_ref` | 类型引用（返回类型、参数类型或字段类型中引用） |
| `inherits` | 超特征约束（`trait Child: Base`） |

## 支持的语言

- **Rust**（通过 `tree-sitter-rust`）

核心设计与语言无关。添加新语言只需实现 `LanguageExtractor` trait 并提供对应的 tree-sitter 语法。

## 设计原则

- **结构化，而非语义化** —— tree-sitter 解析语法而非类型。函数调用解析基于名称匹配，不进行类型推断，也不支持跨 crate 解析。
- **为 LLM 优化** —— 最小化 token 数量，确定性 ID，扁平 JSON 结构。专为智能体遍历设计，而非人工阅读。
- **优雅降级** —— 部分解析时尽量提取可用内容。解析失败的文件会输出警告并跳过。无法解析的跨文件引用会被静默丢弃。

## 开发

```bash
cargo build          # 构建
cargo test           # 运行所有测试（39 个单元测试 + 集成测试）
cargo clippy         # 代码检查
cargo fmt            # 代码格式化
cargo run -- src/    # 对自身源码运行
```

## 许可证

MIT
