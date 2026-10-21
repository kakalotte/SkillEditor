# 0002 — YAML 解析与 frontmatter 无损往返

- 状态：已接受
- 日期：2026-10-17

## 背景

`SKILL.md` 的 frontmatter 是 YAML。产品有一条硬性要求（规格 §6、§10.2）：

> 不能静默删除未知字段。

这意味着解析不能只做「反序列化到固定结构体再序列化回去」，因为那会丢掉所有
未被结构体声明的键、注释、键顺序和原始标量风格。

## 决策

### 1. 用 `serde_yaml_ng`，不用 `serde_yaml`

`serde_yaml` 已归档停更。`serde_yaml_ng`（0.10）是社区维护的延续版本，
API 兼容，且暴露 `Value` / `Mapping`（保持键顺序）。

### 2. 双层模型：`Value` 为真相，结构体为视图

```rust
pub struct Frontmatter {
    raw: serde_yaml_ng::Value,        // 完整、保序、含未知键 —— 真相
    doc: SkillFrontmatter,            // 已知字段的强类型视图，只读
    text: String,                     // 原始 YAML 文本，解析失败时用于恢复
}
```

- 读取：`text → Value`，再从 `Value` 里**取**已知字段填进结构体。
- 写入已知字段：改 `Value` 里的键，其余原样不动。
- 输出：`Value → text` 重新序列化（因此注释会丢；见下）。

### 3. 解析失败不丢数据

frontmatter 语法错误时：

- `parse_ok = false`，`raw` 为空，但**保留完整原始文本**（`text`）。
- 正文与原始 frontmatter 都原封不动交给界面，用户可以在源码视图里直接改。
- **修复成功之前禁止保存**（`save` 返回错误而不是写出半成品）。
- 编辑器侧栏的 name/description 输入框变为只读。

坏 frontmatter 不只是原样打开：用户可以在应用内把 YAML 修好后继续保存。

### 4. 名称校验与规范化分离

`naming::validate_skill_name` 只做校验，返回结构化的失败原因（空、超长、
非法字符、保留名、路径分隔符等），**不做静默改写**。目录名由用户决定；
`save` 在创建新目录前会校验名称并拒绝已存在的目录。

### 5. 中文与 Unicode

序列化统一 UTF-8、不转义非 ASCII；测试覆盖中文名称/描述/正文与 emoji。

## 已知取舍

- **注释会丢**。YAML 注释不进入 `Value`，保存后消失。产品未要求保留注释，
  且这是所有基于 `serde_yaml*` 的实现的共同限制；文档中如实说明，不假装保留。
- **标量风格可能变**。例如 `description: >` 折叠块在保存后可能变成引号字符串。
  `Value` 保留的是语义而非书写风格。
- 未知**键**（含嵌套结构）严格保留，这是硬要求，有测试覆盖
  （`editing_through_the_app_layer_preserves_unknown_fields`）。

## 测试

- `crates/skillforge-core/tests/skill_roundtrip.rs`：正常往返、未知字段保留、
  中文与 Unicode、空正文、超长正文、CRLF/LF。
- `crates/skillforge-core/tests/validation.rs`：frontmatter 相关规则分级。
- `src-tauri/src/app.rs::tests::broken_frontmatter_can_be_repaired_inside_the_app`：
  端到端——导入坏文件 → 确认只读 → 在应用内修复 → 保存成功。
