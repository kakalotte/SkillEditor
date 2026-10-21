//! Read-only reference data for `SKILL.md` frontmatter and validation rules.
//!
//! The desktop UI asks the app layer for this catalogue instead of hard-coding
//! field knowledge in JavaScript. Platform support and listing limits are added
//! by the app layer from the live adapters, so this module only owns the
//! specification text and examples.

use crate::domain::naming::MAX_NAME_LEN;
use crate::platforms::common::SPEC_FIELDS;
use crate::services::validation_service::{
    MAX_COMPATIBILITY_LEN, MAX_DESCRIPTION_LEN, RECOMMENDED_MAX_BODY_LINES,
    RECOMMENDED_MAX_BODY_TOKENS,
};

pub const SPEC_URL: &str = "https://agentskills.io/specification";
pub const CLAUDE_URL: &str = "https://code.claude.com/docs/en/skills";
pub const DSH_URL: &str =
    "https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/skills.md";

/// Where a field belongs in the guide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldCategory {
    Required,
    SpecOptional,
    Platform,
}

impl FieldCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            FieldCategory::Required => "required",
            FieldCategory::SpecOptional => "spec-optional",
            FieldCategory::Platform => "platform",
        }
    }
}

/// One documented frontmatter field.
#[derive(Debug, Clone, Copy)]
pub struct FieldDoc {
    pub key: &'static str,
    /// Short Chinese label shown next to the key.
    pub label: &'static str,
    /// Human-readable YAML type, e.g. "字符串" or "映射".
    pub value_type: &'static str,
    pub required: bool,
    pub category: FieldCategory,
    pub summary: &'static str,
    pub rules: &'static [&'static str],
    pub example: &'static str,
    /// Empty when there is no useful warning to show.
    pub common_mistake: &'static str,
    pub docs_url: &'static str,
    /// Other spellings/aliases, if any.
    pub aliases: &'static [&'static str],
}

pub const FIELDS: &[FieldDoc] = &[
    FieldDoc {
        key: "name",
        label: "名称",
        value_type: "字符串",
        required: true,
        category: FieldCategory::Required,
        summary: "Skill 的唯一名称，也是安装后的目录名和命令名。",
        rules: &[
            "必填，1–64 个字符。",
            "只能使用小写字母、数字和单个连字符；不能以连字符开头或结尾，也不能出现 `--`。",
            "必须与 Skill 所在目录名一致。",
            "安装时会成为目标平台中的目录名（或命令名）。",
        ],
        example: "name: pdf-tools",
        common_mistake: "`PDF_Tools`、`-pdf`、`pdf--tools` 都不合法。",
        docs_url: SPEC_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "description",
        label: "描述",
        value_type: "字符串",
        required: true,
        category: FieldCategory::Required,
        summary: "说明 Skill 做什么、何时使用，以及用户可能提到的触发词。",
        rules: &[
            "必填，1–1024 个字符（按 Unicode 字符计数）。",
            "建议写成「做什么 + 何时使用 + 关键触发词」，让 Agent 能判断是否加载。",
            "平台列表会截断：Claude Code 1536、OpenCode/通用 1024、DeepSeek Harness 500（含 when_to_use / whenToUse）。",
        ],
        example: "description: Extract text and tables from PDF files. Use when the user needs to read, convert, fill, or merge PDFs.",
        common_mistake: "只写 `Helps with PDFs` 太短，Agent 难以判断何时使用。",
        docs_url: SPEC_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "license",
        label: "许可证",
        value_type: "字符串",
        required: false,
        category: FieldCategory::SpecOptional,
        summary: "声明 Skill 使用的许可证，或指向目录内的许可证文件。",
        rules: &[
            "可选；建议只写许可证名称或 LICENSE 文件路径。",
            "Claude Code 接受该字段，但不会据此改变运行行为。",
        ],
        example: "license: MIT",
        common_mistake: "不要把整段许可证正文塞进 frontmatter，放到独立文件即可。",
        docs_url: SPEC_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "compatibility",
        label: "环境要求",
        value_type: "字符串",
        required: false,
        category: FieldCategory::SpecOptional,
        summary: "记录运行 Skill 所需的系统、依赖或网络条件。",
        rules: &[
            "可选，最长 500 个字符。",
            "只写必要的环境要求，例如 Python 版本、系统包、网络访问。",
            "大多数 Skill 不需要这个字段。",
        ],
        example: "compatibility: Requires Python 3.11+ and the pypdf package.",
        common_mistake: "把安装步骤或完整 README 写进 compatibility，应该放到正文或 references/。",
        docs_url: SPEC_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "metadata",
        label: "自定义元数据",
        value_type: "映射（string -> string）",
        required: false,
        category: FieldCategory::SpecOptional,
        summary: "存放平台或团队自定义的信息，不影响 Agent 的加载判断。",
        rules: &[
            "必须是 YAML 映射，不是字符串或列表。",
            "规范建议键名尽量唯一，值使用字符串。",
            "未知字段和 metadata 中的内容会原样保留，安装时不会擅自删除。",
        ],
        example: "metadata:\n  author: example-org\n  version: \"1.0\"",
        common_mistake: "写成 `metadata: internal`（字符串）会触发 `SPEC-META-001`。",
        docs_url: SPEC_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "allowed-tools",
        label: "预授权工具",
        value_type: "字符串（空格分隔）",
        required: false,
        category: FieldCategory::SpecOptional,
        summary: "声明 Skill 运行时可预先批准使用的工具。",
        rules: &[
            "可选，实验性字段；不同 Agent 的支持程度不同。",
            "规范写法是空格分隔的工具名，例如 `Read Bash(git:*)`。",
            "Claude Code 也接受逗号分隔字符串或 YAML 列表。",
        ],
        example: "allowed-tools: Bash(git:*) Bash(jq:*) Read",
        common_mistake: "写成 YAML 列表在部分平台可能不生效；跨平台时优先用空格分隔字符串。",
        docs_url: SPEC_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "when_to_use",
        label: "何时使用（Claude Code）",
        value_type: "字符串",
        required: false,
        category: FieldCategory::Platform,
        summary: "Claude Code 的补充触发说明，会追加到 description 后并计入 1536 字符上限。",
        rules: &[
            "只在 Claude Code 中读取。",
            "适合放触发短语、示例请求，不要重复 description 的全部内容。",
            "安装到 DeepSeek Harness 时，安装副本会自动改为 `whenToUse`。",
        ],
        example: "when_to_use: Use when the user asks to extract a table from a PDF or fill a form.",
        common_mistake: "同时写 `when_to_use` 和 `whenToUse` 时不会自动合并；源文件应只保留一个。",
        docs_url: CLAUDE_URL,
        aliases: &["whenToUse"],
    },
    FieldDoc {
        key: "whenToUse",
        label: "何时使用（Harness）",
        value_type: "字符串",
        required: false,
        category: FieldCategory::Platform,
        summary: "DeepSeek Harness 读取的 camelCase 字段，作用类似 Claude Code 的 when_to_use。",
        rules: &[
            "DeepSeek Harness 只读取 camelCase 的 `whenToUse`。",
            "源文件中的 `when_to_use` 在安装到 Harness 时会被投影为 `whenToUse`（只改安装副本）。",
            "与 description 合计受 Harness 默认 500 字符列表上限约束。",
        ],
        example: "whenToUse: Use when the user asks to extract a table from a PDF.",
        common_mistake: "在 Harness 专用文件中写 `when_to_use` 会被忽略；请使用 `whenToUse`。",
        docs_url: DSH_URL,
        aliases: &["when_to_use"],
    },
    FieldDoc {
        key: "argument-hint",
        label: "参数提示",
        value_type: "字符串",
        required: false,
        category: FieldCategory::Platform,
        summary: "在命令补全中提示 Skill 需要的参数格式。",
        rules: &["仅 Claude Code 读取。", "常见写法：`[issue-number]`、`[filename] [format]`。"],
        example: "argument-hint: \"[issue-number]\"",
        common_mistake: "这里只影响补全提示，不会解析或校验实际参数。",
        docs_url: CLAUDE_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "arguments",
        label: "命名参数",
        value_type: "字符串或列表",
        required: false,
        category: FieldCategory::Platform,
        summary: "为正文中的 `$name` 替换声明按位置对应的命名参数。",
        rules: &[
            "仅 Claude Code 读取。",
            "接受空格分隔字符串或 YAML 列表；名称按位置映射。",
        ],
        example: "arguments: \"target-path format\"",
        common_mistake: "参数名只是占位符，不会自动做类型转换或必填校验。",
        docs_url: CLAUDE_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "disable-model-invocation",
        label: "禁止模型自动调用",
        value_type: "布尔",
        required: false,
        category: FieldCategory::Platform,
        summary: "设为 true 后，只能由用户显式调用，模型不会自动加载。",
        rules: &[
            "Claude Code 与 DeepSeek Harness 都读取该字段。",
            "适合部署、发布等需要人工确认的流程。",
            "布尔值可写 true/false；Claude Code 也接受 yes/no/on/off/1/0。",
        ],
        example: "disable-model-invocation: true",
        common_mistake: "不是所有平台都支持；跨平台使用时仍要在正文里写清人工触发条件。",
        docs_url: CLAUDE_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "user-invocable",
        label: "允许用户调用",
        value_type: "布尔",
        required: false,
        category: FieldCategory::Platform,
        summary: "设为 false 时，Skill 只作为后台知识由模型使用，不出现在用户命令菜单。",
        rules: &[
            "Claude Code 与 DeepSeek Harness 都读取该字段。",
            "默认值为 true。",
        ],
        example: "user-invocable: false",
        common_mistake: "设为 false 不等于禁止模型使用；要限制模型调用请用 disable-model-invocation。",
        docs_url: CLAUDE_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "disallowed-tools",
        label: "禁用工具",
        value_type: "字符串或列表",
        required: false,
        category: FieldCategory::Platform,
        summary: "Skill 激活期间从模型可用工具中移除指定工具。",
        rules: &[
            "仅 Claude Code 读取。",
            "接受空格/逗号分隔字符串或 YAML 列表。",
        ],
        example: "disallowed-tools: AskUserQuestion",
        common_mistake: "它是禁用而不是预授权；不要和 allowed-tools 混用表达同一个意图。",
        docs_url: CLAUDE_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "model",
        label: "模型覆盖",
        value_type: "字符串",
        required: false,
        category: FieldCategory::Platform,
        summary: "Skill 激活时临时切换模型；`inherit` 表示沿用当前模型。",
        rules: &[
            "仅 Claude Code 读取，覆盖只在当前轮次生效。",
            "可写 `inherit` 或平台支持的模型值。",
            "组织级模型白名单可能使覆盖失效。",
        ],
        example: "model: inherit",
        common_mistake: "不要把凭据或密钥写在这里；模型名称不是密钥配置。",
        docs_url: CLAUDE_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "effort",
        label: "推理强度",
        value_type: "字符串",
        required: false,
        category: FieldCategory::Platform,
        summary: "Skill 激活时覆盖会话的推理强度。",
        rules: &[
            "仅 Claude Code 读取。",
            "常见值：low、medium、high、xhigh、max；可用值取决于模型。",
        ],
        example: "effort: high",
        common_mistake: "不是所有模型都支持全部档位；不可用时平台会沿用原设置。",
        docs_url: CLAUDE_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "context",
        label: "执行上下文",
        value_type: "字符串",
        required: false,
        category: FieldCategory::Platform,
        summary: "设为 `fork` 时在独立子 Agent 上下文中运行 Skill。",
        rules: &["仅 Claude Code 读取。", "目前常用值为 `fork`；与 agent 字段配合使用。"],
        example: "context: fork",
        common_mistake: "普通 Skill 不需要该字段；只有需要隔离上下文或子 Agent 时才使用。",
        docs_url: CLAUDE_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "agent",
        label: "子 Agent 类型",
        value_type: "字符串",
        required: false,
        category: FieldCategory::Platform,
        summary: "当 `context: fork` 时指定使用哪种子 Agent。",
        rules: &["仅 Claude Code 读取。", "只有设置 `context: fork` 时才有意义。"],
        example: "agent: general-purpose",
        common_mistake: "省略 context 时该字段不会让 Skill 自动进入子 Agent。",
        docs_url: CLAUDE_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "background",
        label: "后台运行",
        value_type: "布尔",
        required: false,
        category: FieldCategory::Platform,
        summary: "控制 fork 子 Agent 是否在后台运行。",
        rules: &[
            "仅 Claude Code 读取。",
            "只在 `context: fork` 时生效；默认 true。",
        ],
        example: "background: false",
        common_mistake: "普通 Skill 设置该字段没有效果。",
        docs_url: CLAUDE_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "hooks",
        label: "Skill 钩子",
        value_type: "映射",
        required: false,
        category: FieldCategory::Platform,
        summary: "Skill 激活时注册、并在会话余下时间保持运行的钩子配置。",
        rules: &[
            "仅 Claude Code 读取。",
            "结构是 YAML 映射；钩子类型和事件名必须匹配 Claude Code 的 hooks 配置。",
        ],
        example: "hooks:\n  PreToolUse:\n    - matcher: Bash\n      hooks:\n        - type: command\n          command: ./scripts/check.sh",
        common_mistake: "钩子会执行本地命令，只应在可信 Skill 中配置。",
        docs_url: CLAUDE_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "paths",
        label: "路径触发",
        value_type: "字符串或列表",
        required: false,
        category: FieldCategory::Platform,
        summary: "限制 Skill 只在匹配指定文件模式时自动激活。",
        rules: &[
            "仅 Claude Code 读取。",
            "接受逗号分隔字符串或 YAML 列表，格式同路径规则 glob。",
        ],
        example: "paths: \"src/**/*.ts\"",
        common_mistake: "路径模式是自动触发条件，不是文件访问权限。",
        docs_url: CLAUDE_URL,
        aliases: &[],
    },
    FieldDoc {
        key: "shell",
        label: "命令解释器",
        value_type: "字符串",
        required: false,
        category: FieldCategory::Platform,
        summary: "指定 Skill 内联 `!` 命令使用的 shell。",
        rules: &[
            "仅 Claude Code 读取。",
            "接受 `bash`（默认）或 `powershell`。",
        ],
        example: "shell: bash",
        common_mistake: "PowerShell 需要对应工具支持，不是所有环境都能直接使用。",
        docs_url: CLAUDE_URL,
        aliases: &[],
    },
];

/// One validation rule in the guide.
#[derive(Debug, Clone, Copy)]
pub struct RuleDoc {
    pub id: &'static str,
    pub severity: &'static str,
    pub source: &'static str,
    pub source_label: &'static str,
    pub summary: &'static str,
    pub fix: &'static str,
    pub docs_url: &'static str,
}

pub const RULES: &[RuleDoc] = &[
    RuleDoc {
        id: "SPEC-FM-001",
        severity: "error",
        source: "spec",
        source_label: "规范",
        summary: "frontmatter 无法解析；文件会以原始文本模式打开。",
        fix: "检查 `---` 之间的 YAML 语法，修复后重新校验。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "SPEC-FM-002",
        severity: "error",
        source: "spec",
        source_label: "规范",
        summary: "缺少 YAML frontmatter。",
        fix: "在文件开头添加 `---`、`name`、`description`，并以 `---` 结束。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "SPEC-NAME-001",
        severity: "error",
        source: "spec",
        source_label: "规范",
        summary: "缺少必需字段 `name`。",
        fix: "添加 `name`，例如 `name: my-skill`。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "SPEC-NAME-002",
        severity: "error",
        source: "spec",
        source_label: "规范",
        summary: "`name` 不符合命名规范。",
        fix: "使用 1–64 个小写字母、数字和单个连字符，且不以连字符开头或结尾。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "SPEC-NAME-003",
        severity: "error",
        source: "spec",
        source_label: "规范",
        summary: "`name` 与 Skill 所在目录名不一致。",
        fix: "让 `name` 与目录名一致，或重命名目录。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "SPEC-DESC-001",
        severity: "error",
        source: "spec",
        source_label: "规范",
        summary: "缺少必需字段 `description`。",
        fix: "添加说明「做什么」和「何时使用」的 description。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "SPEC-DESC-002",
        severity: "error",
        source: "spec",
        source_label: "规范",
        summary: "`description` 超过 1024 个字符。",
        fix: "精简 description，把细节移到正文或 references/。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "SPEC-COMP-001",
        severity: "error",
        source: "spec",
        source_label: "规范",
        summary: "`compatibility` 超过 500 个字符。",
        fix: "只保留必要的环境要求。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "SPEC-META-001",
        severity: "warning",
        source: "spec",
        source_label: "规范",
        summary: "`metadata` 不是键值映射。",
        fix: "改成 YAML 映射，例如 `metadata:\\n  author: me`。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "SPEC-RES-001",
        severity: "error",
        source: "spec",
        source_label: "规范",
        summary: "资源路径不安全（越出 Skill 根目录或包含 `..`）。",
        fix: "使用 Skill 根目录内的相对路径。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "PLAT-<ID>-FIELD",
        severity: "warning",
        source: "platform",
        source_label: "平台",
        summary: "使用了目标平台不认识的字段；安装后该字段会被忽略。",
        fix: "跨平台时把平台专属内容移入 `metadata`，或接受安装后的忽略。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "PLAT-<ID>-DESC",
        severity: "warning",
        source: "platform",
        source_label: "平台",
        summary: "description（含 when_to_use / whenToUse）超过该平台的列表上限，会被截断。",
        fix: "精简 description，把细节放到正文或 references/。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "QUAL-DESC-001",
        severity: "info",
        source: "quality",
        source_label: "质量",
        summary: "description 偏短，Agent 可能不易判断何时使用。",
        fix: "补充「做什么」和「何时使用」，并包含关键触发词。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "QUAL-DESC-002",
        severity: "info",
        source: "quality",
        source_label: "质量",
        summary: "description 未明确说明「何时使用」。",
        fix: "补充触发场景，例如「当用户需要…时使用」。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "QUAL-BODY-001",
        severity: "warning",
        source: "quality",
        source_label: "质量",
        summary: "正文为空，Skill 没有任何指令内容。",
        fix: "在 frontmatter 之后写下步骤或规则。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "QUAL-BODY-002",
        severity: "info",
        source: "quality",
        source_label: "质量",
        summary: "正文较短，可能不足以稳定复现流程。",
        fix: "补充步骤、边界与示例。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "QUAL-BODY-003",
        severity: "info",
        source: "quality",
        source_label: "质量",
        summary: "正文超过建议的 500 行。",
        fix: "把细节拆分到 references/，正文保持精简。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "QUAL-BODY-004",
        severity: "info",
        source: "quality",
        source_label: "质量",
        summary: "正文约 tokens 超过建议的 5000。",
        fix: "把长篇参考内容移到独立文件，按需加载。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "QUAL-STRUCT-001",
        severity: "info",
        source: "quality",
        source_label: "质量",
        summary: "正文缺少明显的步骤或章节结构。",
        fix: "使用标题或编号步骤组织流程（建议，不是强制规范）。",
        docs_url: SPEC_URL,
    },
    RuleDoc {
        id: "QUAL-STRUCT-002",
        severity: "info",
        source: "quality",
        source_label: "质量",
        summary: "正文中未发现示例。",
        fix: "添加输入/输出示例或边界情况说明。",
        docs_url: SPEC_URL,
    },
];

pub const DIRECTORY_RULES: &[&str] = &[
    "`SKILL.md` 必需，且必须位于 Skill 根目录。",
    "`scripts/`、`references/`、`assets/` 是推荐约定，可选；其他文件原样保留。",
    "应用不会执行脚本，只做复制、统计和资源路径安全校验。",
    "资源路径必须是 Skill 根目录内的相对路径，不能是绝对路径，也不能包含 `..`。",
];

pub const BODY_RULES: &[&str] = &[
    "正文是 frontmatter 之后的 Markdown；Agent 决定使用 Skill 时会整体加载。",
    "推荐结构：何时使用、步骤、注意事项、示例。推荐章节缺失只会给建议，不是规范错误。",
    "建议控制在 500 行 / 约 5000 tokens 以内；超长内容放到 `references/` 按需加载。",
    "引用文件时使用相对路径，尽量保持一层深度，避免深层链式引用。",
    "Claude Code 的 `!` 命令注入等能力是平台扩展，跨平台时不要假设一定可用。",
];

pub const FULL_EXAMPLE: &str = r#"---
name: pdf-tools
description: Extract PDF text and tables, fill forms, and merge files. Use when the user works with PDF documents.
license: MIT
compatibility: Requires Python 3.11+ and the pypdf package.
metadata:
  author: example-org
  version: "1.0"
allowed-tools: Bash(python:*) Read
---

## 何时使用

当用户需要读取、转换、填写或合并 PDF 文件时使用。

## 步骤

1. 确认输入文件路径和期望输出。
2. 使用 `scripts/extract.py` 提取文本或表格。
3. 检查结果，并在必要时报告无法解析的页面。

## 注意事项

- 不要覆盖原始文件；输出写到用户指定的位置。
- 扫描版 PDF 可能需要 OCR，先说明这一限制。

## 示例

输入：`report.pdf`，输出：Markdown 文本和表格。
"#;

/// Limits exposed to the UI so it does not have to duplicate constants.
pub const NAME_MAX_LEN: usize = MAX_NAME_LEN;
pub const DESCRIPTION_MAX_LEN: usize = MAX_DESCRIPTION_LEN;
pub const COMPATIBILITY_MAX_LEN: usize = MAX_COMPATIBILITY_LEN;
pub const BODY_MAX_LINES: usize = RECOMMENDED_MAX_BODY_LINES;
pub const BODY_MAX_TOKENS: usize = RECOMMENDED_MAX_BODY_TOKENS;

/// True when every Agent Skills spec field has a guide entry.
pub fn spec_fields_are_covered() -> bool {
    SPEC_FIELDS
        .iter()
        .all(|key| FIELDS.iter().any(|f| f.key == *key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::skill_md;
    use crate::platforms::all_adapters;
    use std::collections::BTreeSet;

    #[test]
    fn covers_spec_fields_and_every_adapter_field() {
        assert!(spec_fields_are_covered());
        let keys: BTreeSet<&str> = FIELDS.iter().map(|f| f.key).collect();
        for adapter in all_adapters() {
            for key in adapter.capabilities().supported_fields {
                assert!(keys.contains(key), "{} 的字段 {key} 缺少指南", adapter.id());
            }
        }
    }

    #[test]
    fn field_keys_are_unique_and_categorized() {
        let keys: BTreeSet<&str> = FIELDS.iter().map(|f| f.key).collect();
        assert_eq!(keys.len(), FIELDS.len(), "字段 key 不能重复");
        assert_eq!(
            FIELDS
                .iter()
                .filter(|f| f.category == FieldCategory::Required)
                .count(),
            2,
            "必填字段应只有 name 和 description"
        );
    }

    #[test]
    fn every_field_example_is_valid_yaml() {
        for field in FIELDS {
            let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(field.example)
                .unwrap_or_else(|e| panic!("{} 的示例不是合法 YAML: {e}", field.key));
            assert!(
                matches!(value, serde_yaml_ng::Value::Mapping(_)),
                "{} 的示例必须是 YAML 映射",
                field.key
            );
        }
    }

    #[test]
    fn full_example_parses_as_a_skill() {
        let parsed = skill_md::parse(FULL_EXAMPLE).expect("完整示例必须是合法 SKILL.md");
        assert!(parsed.had_block);
        assert_eq!(
            parsed.frontmatter.get_str_lossy("name").as_deref(),
            Some("pdf-tools")
        );
        assert!(parsed.body.contains("## 何时使用"));
    }

    #[test]
    fn rules_are_unique_and_match_documented_constants() {
        let ids: BTreeSet<&str> = RULES.iter().map(|r| r.id).collect();
        assert_eq!(ids.len(), RULES.len(), "规则 id 不能重复");
        assert_eq!(RULES.len(), 20, "规则清单应与 product-spec 的 20 条一致");
        assert_eq!(NAME_MAX_LEN, 64);
        assert_eq!(DESCRIPTION_MAX_LEN, 1024);
        assert_eq!(COMPATIBILITY_MAX_LEN, 500);
        assert_eq!(BODY_MAX_LINES, 500);
        assert_eq!(BODY_MAX_TOKENS, 5000);
    }
}
