# 0005 — 诊断：不引入日志框架，统一写 stderr

- 状态：已接受
- 日期：2026-10-17

## 背景

GUI 应用最容易出的问题是「窗口起来了但界面空白」或「点按钮没反应」。
没有日志时，这类问题在打包后的 `.app` 里几乎无法定位：既没有终端输出，
也不一定方便开 WebView 的开发者工具。

选项有三：

1. 引入 `log` + `env_logger`/`tracing`（Rust 生态常规做法）。
2. 把前端错误写进日志文件。
3. 不引入框架，前端诊断经一条命令转发到 stderr。

## 决策

选 3。理由：

- 规格 §13 要求避免过度架构，§10.1 要求「不在日志中记录完整正文」。
  一个完整的日志框架会带来级别/过滤器/轮转/输出目标等一整批配置面，
  而本产品只需要「出问题时能看到发生了什么」。
- 本实现沿用「诊断写 stderr、不引入日志框架」的原则。
- stderr 是终端启动时唯一可靠的通道，也是打包后 `open` 之前
  （直接运行 `Contents/MacOS/skillforge`）唯一能看到的通道。

### 实现

```rust
#[tauri::command]
pub fn log_frontend(level: String, message: String) {
    let label = match level.as_str() {
        "error" => "错误",
        "warn" => "警告",
        _ => "信息",
    };
    eprintln!("[前端/{label}] {message}");
}
```

前端（`app.js`）：

```js
function log(level, message) {
  invoke("log_frontend", { level, message }).catch(() => {});
}
```

### 什么时候写

刻意保持低频，避免刷屏：

1. **首次渲染完成**：`界面渲染完成：技能 N，平台 M，目标 K，主题 T`
   —— 这一行能证明四个页面的构建与渲染函数全部执行完毕。
2. **启动完成**：`已就绪：N 个 Skill，工作区 <path>`。
3. **`boot` 失败**：明确报告初始状态获取失败。
4. **未捕获异常**（`window.onerror`）：含消息、文件名、行号。
5. **未处理的 Promise 拒绝**（`unhandledrejection`）。

**不写**：每次交互、每次快照、任何 Skill 正文内容。

### 为什么这既是功能也是验证手段

在没有可靠截图能力的环境里，「前端是否真的跑起来了」只能靠可观测信号。
本机制给出的证据链是：

```
窗口创建（lsappinfo 显示 type="Foreground"，WebKit GPU/网络/WebContent XPC 存在）
  → WebView 加载了嵌入的 index.html
    → app.js 执行到 main()
      → boot 命令跨 IPC 成功（返回了 Snapshot）
        → applySnapshot 中 4 个 render* 函数全部无异常
          → stderr 出现「界面渲染完成」和「已就绪」
            → 且工作区目录被真实创建到磁盘
```

任何一环断裂，stderr 上都会出现「前端/错误」而不是「界面渲染完成」。

### 验证记录

```
$ HOME=$(mktemp -d) ./dist/SkillForge.app/Contents/MacOS/skillforge 2>&1
[前端/信息] 界面渲染完成：技能 1，平台 6，目标 7，主题 system
[前端/信息] 已就绪：1 个 Skill，工作区 <tmp>/Library/Application Support/SkillForge/workspace
```

（工作区内预置了 1 个正常 Skill 和 1 个 frontmatter 损坏的 Skill，两者都被列出，
后者未被静默丢弃。）

## 后果

- ✅ 打包后无开发者工具也能定位前端故障。
- ✅ 零依赖、零配置。
- ✅ 让「界面真的渲染了」成为可断言的证据。
- ⚠️ 不是结构化日志，没有级别过滤、没有轮转、不写文件。
  本项目不需要这些；如果将来需要，应新开 ADR。
- ⚠️ 不记录 Skill 正文（符合 §10.1）。错误消息若来自 Rust
  只会包含路径与校验信息，不含内容。
