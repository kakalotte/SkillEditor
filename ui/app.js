/* SkillForge front end ------------------------------------------------------
 *
 * Deliberately plain: no framework, no bundler, no npm. The Rust side sends a
 * complete "snapshot" for every state-changing command and the code below only
 * renders it, so no business logic lives in JavaScript. That keeps the whole
 * app buildable with `cargo build` alone.
 *
 * Two rules matter for correctness and safety:
 *   1. All values coming from the core (skill names, descriptions, paths,
 *      validation messages) are inserted with `textContent`, never with
 *      `innerHTML`. A SKILL.md is untrusted input; it must never become markup.
 *   2. Text fields are debounced, and every command that reads the document
 *      flushes the pending edits first, so the Rust state can never be stale.
 */

"use strict";

(function () {
  const TAURI = window.__TAURI__;
  if (!TAURI || !TAURI.core) {
    document.body.textContent = "SkillForge 需要运行在 Tauri 窗口中。";
    return;
  }

  const invoke = TAURI.core.invoke;
  const listen = TAURI.event ? TAURI.event.listen : null;

  const state = {
    snapshot: null,
    page: "skills",
    editorMode: "source",
    preview: null,
    localStatus: null,
    editing: { name: null, description: null, body: null },
    timer: 0,
    renderedDocId: null,
    confirmAction: null,
    guide: null,
    guidePromise: null,
    guideTab: "required",
    guideQuery: "",
  };

  const refs = {};

  const GUIDE_TABS = [
    { id: "required", label: "必填字段" },
    { id: "optional", label: "规范可选" },
    { id: "platform", label: "平台扩展" },
    { id: "body", label: "正文与目录" },
    { id: "rules", label: "校验规则" },
  ];

  // ---------------------------------------------------------------------
  // DOM helpers (no innerHTML with data, ever)
  // ---------------------------------------------------------------------

  const $ = (id) => document.getElementById(id);

  function text(value) {
    return document.createTextNode(value == null ? "" : String(value));
  }

  function append(node, children) {
    if (children == null) return;
    const list = Array.isArray(children) ? children : [children];
    for (const child of list) {
      if (child == null || child === false) continue;
      node.appendChild(
        typeof child === "string" || typeof child === "number"
          ? text(child)
          : child
      );
    }
  }

  function el(tag, props, children) {
    const node = document.createElement(tag);
    if (props) {
      for (const [key, value] of Object.entries(props)) {
        if (value == null || value === false) continue;
        if (key === "class") node.className = value;
        else if (key === "text") node.textContent = String(value);
        else if (key === "dataset") Object.assign(node.dataset, value);
        else if (key.startsWith("on") && typeof value === "function")
          node.addEventListener(key.slice(2).toLowerCase(), value);
        else if (value === true) node.setAttribute(key, "");
        else node.setAttribute(key, String(value));
      }
    }
    append(node, children);
    return node;
  }

  function clear(node) {
    while (node.firstChild) node.removeChild(node.firstChild);
  }

  function badge(label, cls) {
    return el("span", { class: "badge " + cls }, [text(label)]);
  }

  function field(label, control) {
    return el("div", { class: "field" }, [el("label", { text: label }), control]);
  }

  function fieldWithHint(label, control, hint) {
    return el("div", { class: "field" }, [
      el("label", { text: label }),
      control,
      hint,
    ]);
  }

  function button(label, cls, onclick, extra) {
    return el(
      "button",
      Object.assign({ type: "button", class: cls || "btn", onclick }, extra || {}),
      [text(label)]
    );
  }

  // ---------------------------------------------------------------------
  // IPC
  // ---------------------------------------------------------------------

  async function call(command, args) {
    try {
      return await invoke(command, args || {});
    } catch (error) {
      showStatus("error", String(error));
      return null;
    }
  }

  /**
   * Mirror a message to stderr through the Rust side. There is no logging
   * framework in this build, so this is how a broken render or a rejected
   * promise is diagnosable when the packaged app is started from a terminal.
   */
  function log(level, message) {
    invoke("log_frontend", { level, message }).catch(() => {});
  }

  /**
   * Run a command that returns a `Snapshot`, then render it.
   *
   * Only commands returning `Snapshot` may go through here. Commands returning
   * anything else (`preview_install`, `preview_uninstall`, `preview_text`, the
   * `set_*` edit acks, side effects) must use `call()`: passing their result to
   * `applySnapshot` would replace `state.snapshot` with a shape the renderers
   * cannot read. The shape check below keeps such a mistake from corrupting the
   * whole UI; `frontend_contract.rs` enforces the same rule statically.
   */
  async function run(command, args) {
    const snapshot = await call(command, args);
    if (!snapshot) return null;
    if (!Array.isArray(snapshot.skills)) {
      const message = `命令 ${command} 没有返回 Snapshot；返回其他类型时必须用 call() 调用`;
      showStatus("error", message);
      log("error", message);
      return null;
    }
    applySnapshot(snapshot);
    return snapshot;
  }

  function showStatus(kind, message) {
    state.localStatus = { kind, message };
    renderStatus();
  }

  // ---------------------------------------------------------------------
  // Theme
  // ---------------------------------------------------------------------

  const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");

  function applyTheme(preference) {
    const dark =
      preference === "dark" || (preference !== "light" && darkQuery.matches);
    document.documentElement.setAttribute("data-theme", dark ? "dark" : "light");
  }

  darkQuery.addEventListener("change", () => {
    applyTheme(state.snapshot ? state.snapshot.theme : "system");
  });

  // ---------------------------------------------------------------------
  // Snapshot application
  // ---------------------------------------------------------------------

  function applySnapshot(snapshot) {
    if (!snapshot) return;
    const previousId =
      state.snapshot && state.snapshot.editor ? state.snapshot.editor.id : null;
    const nextId = snapshot.editor ? snapshot.editor.id : null;
    const docChanged = previousId !== nextId;

    state.snapshot = snapshot;
    state.localStatus = null;
    if (docChanged) {
      state.editing = { name: null, description: null, body: null };
      state.preview = null;
      state.renderedDocId = null;
    }

    applyTheme(snapshot.theme);
    renderTop();
    renderStatus();
    renderSkills();
    renderEditor(docChanged);
    renderPlatforms();
    renderSettings();

    if (!state.loggedRender) {
      state.loggedRender = true;
      log(
        "info",
        `界面渲染完成：技能 ${snapshot.skills.length}，平台 ${snapshot.platforms.length}，目标 ${snapshot.targets.length}，主题 ${snapshot.theme}`
      );
    }
  }

  /** Apply an `EditAck` without rebuilding the fields the user is typing in. */
  function applyAck(ack) {
    if (!ack || !state.snapshot) return;
    const editor = state.snapshot.editor;
    if (editor) {
      editor.dirty = ack.dirty;
      editor.parseOk = ack.parseOk;
      editor.readOnly = ack.readOnly;
      editor.frontmatterKeys = ack.frontmatterKeys;
      editor.stats = ack.stats;
    }
    state.snapshot.issues = ack.issues;
    state.snapshot.counts = ack.counts;
    renderStatus();
    updateEditorMeta();
    updateEditorSide();
    updateSkillBadges();
  }

  // ---------------------------------------------------------------------
  // Debounced text edits
  // ---------------------------------------------------------------------

  function queueEdit(fieldName, value) {
    state.editing[fieldName] = value;
    if (state.timer) window.clearTimeout(state.timer);
    state.timer = window.setTimeout(flushEdits, 160);
  }

  /**
   * Send pending text edits to the core. Awaited by every command that reads
   * the document, so clicking "保存" straight after typing cannot lose input.
   */
  async function flushEdits() {
    if (state.timer) {
      window.clearTimeout(state.timer);
      state.timer = 0;
    }
    const pending = state.editing;
    state.editing = { name: null, description: null, body: null };

    let ack = null;
    const applied = {};
    if (pending.name !== null) {
      const result = await call("set_name", { value: pending.name });
      if (result) {
        applied.name = pending.name;
        ack = result;
      }
    }
    if (pending.description !== null) {
      const result = await call("set_description", { value: pending.description });
      if (result) {
        applied.description = pending.description;
        ack = result;
      }
    }
    if (pending.body !== null) {
      const result = await call("set_body", { value: pending.body });
      if (result) {
        applied.body = pending.body;
        ack = result;
      }
    }

    // `EditAck` does not echo the text back. Keep the accepted values in the
    // local snapshot so hints/counts do not snap back to the previous text
    // after the debounce clears `state.editing`.
    const editor = state.snapshot && state.snapshot.editor;
    if (editor) {
      if ("name" in applied) editor.name = applied.name;
      if ("description" in applied) editor.description = applied.description;
      if ("body" in applied) editor.body = applied.body;
    }
    if (ack) applyAck(ack);
    return ack;
  }

  /** Flush pending edits, then run a document-reading command. */
  async function runAfterFlush(command, args) {
    await flushEdits();
    return run(command, args);
  }

  // ---------------------------------------------------------------------
  // Top bar, navigation, status bar
  // ---------------------------------------------------------------------

  function renderTop() {
    const snapshot = state.snapshot;
    if (!snapshot) return;
    const workspace = snapshot.workspace || "(未设置)";
    $("workspace-summary").textContent =
      `${snapshot.skills.length} 个 Skill · 工作区 ${workspace}`;
  }

  function renderNav() {
    for (const item of document.querySelectorAll(".nav-item")) {
      item.classList.toggle("active", item.dataset.page === state.page);
    }
    for (const page of document.querySelectorAll(".page")) {
      page.classList.toggle("active", page.id === "page-" + state.page);
    }
  }

  function setPage(page) {
    state.page = page;
    renderNav();
    if (page === "editor") renderEditor(true);
    if (page === "platforms") renderPlatforms();
    if (page === "settings") renderSettings();
  }

  function renderStatus() {
    const snapshot = state.snapshot;
    const status = state.localStatus || (snapshot && snapshot.status) || {
      kind: "info",
      message: "",
    };
    const node = $("status-message");
    node.textContent = status.message;
    node.className = "status " + (status.kind || "info");

    const counts = $("status-counts");
    clear(counts);
    if (snapshot && snapshot.editor) {
      const c = snapshot.counts;
      append(counts, [
        el("span", { class: "count error", text: `${c.errors} 错误` }),
        el("span", { class: "count warning", text: `${c.warnings} 警告` }),
        el("span", { class: "count info", text: `${c.infos} 建议` }),
      ]);
    }

    $("status-version").textContent = snapshot
      ? `v${snapshot.appVersion}`
      : "";
  }

  // ---------------------------------------------------------------------
  // Skills page
  // ---------------------------------------------------------------------

  function buildSkillsShell() {
    const host = $("page-skills");
    clear(host);
    refs.search = el("input", {
      type: "search",
      class: "search",
      placeholder: "搜索名称或描述…",
      oninput: (event) => {
        state.snapshot.search = event.target.value;
        renderSkills();
        window.clearTimeout(refs.searchTimer);
        refs.searchTimer = window.setTimeout(
          () => run("set_search", { text: event.target.value }),
          200
        );
      },
    });
    refs.skillList = el("div", { class: "skill-list" });
    append(host, [
      el("div", { class: "row wrap" }, [
        refs.search,
        el("div", { class: "spacer" }),
        button("新建 Skill", "btn primary", () => run("new_skill").then(openEditorPage)),
        button("导入…", "btn", onImport),
        button("刷新列表", "btn", () => run("refresh_skills")),
      ]),
      refs.skillList,
    ]);
  }

  function renderSkills() {
    const snapshot = state.snapshot;
    if (!snapshot || !refs.skillList) return;
    if (document.activeElement !== refs.search) refs.search.value = snapshot.search;

    clear(refs.skillList);
    if (snapshot.skills.length === 0) {
      append(refs.skillList, [
        el("div", {
          class: "empty",
          text: snapshot.search
            ? "没有匹配的 Skill。"
            : "工作区还是空的：点击「新建 Skill」或「导入…」开始。",
        }),
      ]);
      return;
    }
    for (const skill of snapshot.skills) refs.skillList.appendChild(skillCard(skill));
  }

  /** Cheap update of the dirty/status badges without rebuilding the list. */
  function updateSkillBadges() {
    const snapshot = state.snapshot;
    if (!snapshot || !refs.skillList) return;
    for (const skill of snapshot.skills) {
      const dirty = refs.skillList.querySelector(
        `[data-dirty-for="${cssEscape(skill.id)}"]`
      );
      if (dirty) dirty.hidden = !skill.dirty;
    }
  }

  function cssEscape(value) {
    return String(value).replace(/["\\]/g, "\\$&");
  }

  function statusBadge(status) {
    switch (status) {
      case "installed":
        return badge("已安装", "installed");
      case "outdated":
        return badge("安装已过期", "outdated");
      case "attention":
        return badge("目标被外部占用", "attention");
      default:
        return null;
    }
  }

  function skillCard(skill) {
    const head = el("div", { class: "card-head" }, [
      el("div", { class: "spacer" }, [
        el("div", { class: "card-title", text: skill.name || "(未命名)" }),
        el("div", { class: "card-sub", text: skill.description || "（无描述）" }),
      ]),
      el(
        "span",
        {
          class: "badge dirty",
          hidden: !skill.dirty,
          dataset: { dirtyFor: skill.id },
        },
        [text("未保存")]
      ),
      !skill.parseOk ? badge("frontmatter 无法解析", "error") : null,
      statusBadge(skill.status),
    ].filter(Boolean));

    return el("div", { class: "card" }, [
      head,
      el("div", { class: "skill-meta" }, [
        el("span", { class: "path-chip", text: skill.path }),
        el("span", { text: `修改于 ${skill.modified}` }),
      ]),
      el("div", { class: "row", style: "margin-top:10px" }, [
        button("打开", "btn primary small", () =>
          run("open_skill", { id: skill.id }).then(openEditorPage)
        ),
        button("在文件管理器中显示", "btn small", () =>
          call("reveal_skill", { id: skill.id })
        ),
        el("div", { class: "spacer" }),
        button("删除", "btn danger small", () => askDelete(skill)),
      ]),
    ]);
  }

  function openEditorPage() {
    state.page = "editor";
    renderNav();
    renderEditor(true);
  }

  function askDelete(skill) {
    askConfirm({
      title: `删除 “${skill.name || skill.path}”`,
      message:
        `将递归删除目录：\n${skill.path}\n\n` +
        "该操作不可撤销（工作区外的安装副本不受影响）。",
      confirmLabel: "删除",
      onConfirm: async () => {
        await run("delete_skill", { id: skill.id, confirm: true });
      },
    });
  }

  async function onImport() {
    await runAfterFlush("import_skill");
    if (state.snapshot && state.snapshot.editor) openEditorPage();
  }

  // ---------------------------------------------------------------------
  // Editor page
  // ---------------------------------------------------------------------

  function buildEditorShell() {
    const host = $("page-editor");
    clear(host);

    refs.editorEmpty = el("div", { class: "empty" }, [
      text("还没有打开任何 Skill。到「技能」页打开一个，或新建一个。"),
    ]);

    refs.editorName = el("input", {
      type: "text",
      oninput: (event) => queueEdit("name", event.target.value),
    });
    refs.editorDescription = el("input", {
      type: "text",
      oninput: (event) => {
        queueEdit("description", event.target.value);
        updateDescriptionHint(event.target.value);
      },
    });
    refs.editorNameHint = el("div", { class: "field-hint" });
    refs.editorDescriptionHint = el("div", { class: "field-hint" });
    refs.editorBody = el("textarea", {
      class: "code-area",
      spellcheck: "false",
      oninput: (event) => queueEdit("body", event.target.value),
    });
    refs.editorPreview = el("pre", { class: "preview", hidden: true });
    refs.editorSourceTab = el(
      "button",
      { type: "button", class: "tab", onclick: () => setEditorMode("source") },
      [text("源码")]
    );
    refs.editorPreviewTab = el(
      "button",
      { type: "button", class: "tab", onclick: () => setEditorMode("preview") },
      [text("预览 SKILL.md")]
    );
    refs.editorPath = el("div", { class: "path-chip" });
    refs.editorDirty = el("span", { class: "badge dirty", hidden: true }, [
      text("未保存"),
    ]);
    refs.editorRawWarning = el(
      "div",
      { class: "notice error", hidden: true },
      [
        text(
          "frontmatter 无法解析：已按原始文本打开，可在此直接修复 YAML；修复成功前无法保存。"
        ),
      ]
    );

    refs.issueList = el("div", { class: "issue-list" });
    refs.resourceList = el("div", { class: "chip-list" });
    refs.keyList = el("div", { class: "chip-list" });
    refs.keySummary = el("div", { class: "field-summary" });
    refs.statGrid = el("div", { class: "stat-grid" });

    const form = el("div", { class: "card" }, [
      el("div", { class: "card-head" }, [
        el("div", { class: "spacer" }, [
          el("div", { class: "card-title", text: "SKILL.md" }),
          refs.editorPath,
        ]),
        refs.editorDirty,
      ]),
      refs.editorRawWarning,
      el("div", { class: "stack" }, [
        fieldWithHint("name", refs.editorName, refs.editorNameHint),
        fieldWithHint(
          "description",
          refs.editorDescription,
          refs.editorDescriptionHint
        ),
      ]),
      el("div", { class: "row wrap", style: "margin:10px 0" }, [
        button("保存", "btn primary", () => runAfterFlush("save")),
        button("另存为…", "btn", () => runAfterFlush("save_skill_as")),
        button("导出 ZIP…", "btn", () => runAfterFlush("export_skill")),
        button("校验", "btn", () => runAfterFlush("validate_now")),
        button("Frontmatter 规则", "btn", openGuide),
        button("在文件管理器中显示", "btn", () => {
          const editor = state.snapshot.editor;
          if (editor) call("open_target", { path: editor.path });
        }),
        button("返回列表", "btn", () => setPage("skills")),
      ]),
      el("div", { class: "tabs" }, [refs.editorSourceTab, refs.editorPreviewTab]),
      refs.editorBody,
      refs.editorPreview,
    ]);

    const side = el("div", { class: "stack" }, [
      el("div", { class: "card" }, [
        el("h3", { class: "section-title", text: "校验结果" }),
        refs.issueList,
      ]),
      el("div", { class: "card" }, [
        el("h3", { class: "section-title", text: "结构统计" }),
        refs.statGrid,
      ]),
      el("div", { class: "card" }, [
        el("h3", { class: "section-title", text: "资源文件" }),
        refs.resourceList,
      ]),
      el("div", { class: "card" }, [
        el("h3", { class: "section-title", text: "frontmatter 字段" }),
        el("p", {
          class: "muted",
          style: "margin:0 0 6px",
          text: "未知字段原样保留，不会被删除。",
        }),
        refs.keySummary,
        refs.keyList,
        el("div", { class: "row", style: "margin-top:8px" }, [
          button("查看全部字段与规则", "btn link", openGuide),
        ]),
      ]),
    ]);

    refs.editorCard = el("div", { class: "editor-grid" }, [form, side]);
    append(host, [refs.editorEmpty, refs.editorCard]);
  }

  function renderEditor(docChanged) {
    const snapshot = state.snapshot;
    if (!snapshot || !refs.editorCard) return;
    const editor = snapshot.editor;

    refs.editorEmpty.hidden = Boolean(editor);
    refs.editorCard.hidden = !editor;
    if (!editor) {
      state.renderedDocId = null;
      return;
    }

    const isNewDocument = docChanged || state.renderedDocId !== editor.id;
    if (isNewDocument) {
      state.renderedDocId = editor.id;
      refs.editorName.value = editor.name;
      refs.editorDescription.value = editor.description;
      refs.editorBody.value = editor.body;
      refs.editorPreview.textContent = "";
      state.preview = null;
    }
    setEditorMode(state.editorMode);
    updateEditorMeta();
    updateEditorSide();
  }

  function setEditorMode(mode) {
    state.editorMode = mode;
    if (!refs.editorBody) return;
    refs.editorBody.hidden = mode !== "source";
    refs.editorPreview.hidden = mode !== "preview";
    refs.editorSourceTab.classList.toggle("active", mode === "source");
    refs.editorPreviewTab.classList.toggle("active", mode === "preview");
    if (mode === "preview") loadPreview();
  }

  async function loadPreview() {
    const editor = state.snapshot && state.snapshot.editor;
    if (!editor) return;
    const text_ = await call("preview_text");
    if (typeof text_ === "string") {
      refs.editorPreview.textContent = text_;
    }
  }

  function updateEditorMeta() {
    const editor = state.snapshot && state.snapshot.editor;
    if (!editor || !refs.editorCard) return;
    refs.editorPath.textContent = editor.path;
    refs.editorDirty.hidden = !editor.dirty;
    refs.editorRawWarning.hidden = editor.parseOk;
    refs.editorName.disabled = editor.readOnly;
    refs.editorDescription.disabled = editor.readOnly;
    updateNameHint();
    updateDescriptionHint(currentDescriptionValue());
  }

  function currentDescriptionValue() {
    const editor = state.snapshot && state.snapshot.editor;
    if (!editor) return "";
    return state.editing.description !== null
      ? state.editing.description
      : editor.description;
  }

  function updateNameHint() {
    if (!refs.editorNameHint) return;
    const max = state.guide && state.guide.limits
      ? ` · 最多 ${state.guide.limits.nameMax} 字符`
      : "";
    refs.editorNameHint.textContent =
      `必填${max} · 小写字母、数字和单个连字符 · 必须与目录名一致`;
  }

  function updateDescriptionHint(value) {
    if (!refs.editorDescriptionHint) return;
    const count = Array.from(value || "").length;
    const max = state.guide && state.guide.limits
      ? ` / ${state.guide.limits.descriptionMax}`
      : "";
    refs.editorDescriptionHint.textContent =
      `必填 · 当前 ${count}${max} 字符 · 说明做什么和何时使用`;
  }

  function ensureGuide() {
    if (state.guide) return Promise.resolve(state.guide);
    if (state.guidePromise) return state.guidePromise;
    state.guidePromise = call("frontmatter_guide")
      .then((guide) => {
        if (guide) {
          state.guide = guide;
          updateNameHint();
          updateDescriptionHint(currentDescriptionValue());
          updateEditorSide();
        }
        return guide;
      })
      .finally(() => {
        state.guidePromise = null;
      });
    return state.guidePromise;
  }

  function updateEditorSide() {
    const snapshot = state.snapshot;
    const editor = snapshot && snapshot.editor;
    if (!editor || !refs.issueList) return;

    clear(refs.issueList);
    if (snapshot.issues.length === 0) {
      append(refs.issueList, [el("p", { class: "muted", text: "未发现问题。" })]);
    } else {
      for (const issue of snapshot.issues) append(refs.issueList, [issueRow(issue)]);
    }

    clear(refs.resourceList);
    if (editor.resources.length === 0) {
      append(refs.resourceList, [
        el("span", { class: "muted", text: "没有附带资源文件。" }),
      ]);
    } else {
      for (const resource of editor.resources) {
        append(refs.resourceList, [
          el("span", { class: "chip", text: `${resource.path} · ${resource.kind} · ${resource.size}` }),
        ]);
      }
    }

    clear(refs.keyList);
    if (editor.frontmatterKeys.length === 0) {
      append(refs.keyList, [el("span", { class: "muted", text: "（无 frontmatter）" })]);
    } else {
      for (const key of editor.frontmatterKeys)
        append(refs.keyList, [el("span", { class: "chip", text: key })]);
    }

    clear(refs.keySummary);
    if (!editor.parseOk) {
      append(refs.keySummary, [
        badge("无法读取", "error"),
        el("span", { class: "muted", text: "frontmatter 解析失败，字段状态未知。" }),
      ]);
    } else if (!state.guide) {
      append(refs.keySummary, [
        el("span", { class: "muted", text: "正在加载字段规则…" }),
      ]);
      ensureGuide();
    } else {
      const keys = new Set(editor.frontmatterKeys);
      const required = state.guide.fields.filter((field) => field.required);
      const used = state.guide.fields.filter((field) => keys.has(field.key));
      const missing = required.filter((field) => !keys.has(field.key));
      append(refs.keySummary, [
        badge(
          missing.length ? "缺少必填" : "必填完整",
          missing.length ? "warning" : "ok"
        ),
        el("span", { text: `已使用 ${used.length} 个字段` }),
        missing.length
          ? el("span", {
              class: "muted",
              text: `缺少 ${missing.map((field) => field.key).join("、")}`,
            })
          : null,
        el("span", {
          class: "muted",
          text: `description ${Array.from(currentDescriptionValue()).length} 字符`,
        }),
      ].filter(Boolean));
    }

    clear(refs.statGrid);
    const stats = editor.stats;
    const rows = [
      ["行数", stats.lines],
      ["字符数", stats.characters],
      ["标题", stats.headings],
      ["列表项", stats.listItems],
      ["代码块", stats.codeFences],
      ["链接", stats.links],
      ["≈ tokens", stats.approxTokens],
    ];
    for (const [label, value] of rows) {
      append(refs.statGrid, [
        el("div", { class: "stat" }, [
          el("span", { text: label }),
          el("b", { text: String(value) }),
        ]),
      ]);
    }
  }

  function issueRow(issue) {
    const where = issue.line ? `${issue.file}:${issue.line}` : issue.file;
    return el("div", { class: "issue " + issue.severity }, [
      el("div", { class: "issue-head" }, [
        badge(severityLabel(issue.severity), issue.severity),
        el("span", { class: "mono", text: issue.ruleId }),
        el("span", { class: "issue-where", text: where || "" }),
        el("div", { class: "spacer" }),
        el("span", { class: "muted", text: issue.sourceLabel }),
      ]),
      el("div", { class: "issue-message", text: issue.message }),
      issue.suggestion ? el("div", { class: "issue-hint", text: issue.suggestion }) : null,
      issue.docUrl
        ? el("div", {}, [
            button("查看规则文档", "btn link", () => call("open_url", { url: issue.docUrl })),
          ])
        : null,
    ].filter(Boolean));
  }

  function severityLabel(severity) {
    return severity === "error" ? "错误" : severity === "warning" ? "警告" : "建议";
  }

  // ---------------------------------------------------------------------
  // Platforms page
  // ---------------------------------------------------------------------

  function buildPlatformsShell() {
    const host = $("page-platforms");
    clear(host);
    refs.platformProject = el("div", { class: "path-chip" });
    refs.platformGrid = el("div", { class: "platform-grid" });
    append(host, [
      el("div", { class: "card" }, [
        el("div", { class: "card-head" }, [
          el("div", { class: "spacer" }, [
            el("div", { class: "card-title", text: "平台目录" }),
            el("div", {
              class: "card-sub",
              text: "路径来自官方文档，均可用「设置 → 自定义路径」覆盖；未实测的组合标记为「待验证」。",
            }),
          ]),
          button("重新检测", "btn", () => run("refresh_platforms")),
        ]),
        el("div", { class: "field" }, [
          el("label", { text: "当前项目目录（用于项目级目标发现）" }),
          refs.platformProject,
        ]),
      ]),
      refs.platformGrid,
    ]);
  }

  function renderPlatforms() {
    const snapshot = state.snapshot;
    if (!snapshot || !refs.platformGrid) return;
    refs.platformProject.textContent = snapshot.projectDir || "（未检测到项目目录）";
    clear(refs.platformGrid);

    const targetsFor = (platformId) =>
      snapshot.targets.filter((t) => t.platformId === platformId);

    for (const platform of snapshot.platforms) {
      const rows = [];
      rows.push(
        el("div", { class: "card-head" }, [
          el("div", { class: "spacer" }, [
            el("div", { class: "card-title", text: platform.name }),
            el("div", { class: "card-sub mono", text: platform.id }),
          ]),
          badge(platform.verificationLabel, platform.verification),
          platform.detected ? badge("已检测到", "ok") : badge("未检测到", "neutral"),
        ])
      );

      rows.push(
        el("div", { class: "row wrap" }, [
          el("span", { class: "muted", text: `核实日期 ${platform.verifiedOn}` }),
          button("官方文档", "btn link", () => call("open_url", { url: platform.docsUrl })),
        ])
      );

      if (platform.detectDetail) {
        rows.push(el("div", { class: "muted", text: platform.detectDetail }));
      }

      rows.push(
        el("div", { class: "row wrap", style: "margin-top:6px" }, [
          platform.compatStatus === "unknown"
            ? badge("当前 Skill：未打开", "neutral")
            : platform.compatStatus === "warning"
              ? badge(`当前 Skill：${platform.compatWarnings} 项兼容提醒`, "warning")
              : badge("当前 Skill：兼容", "ok"),
          platform.compatRewrites > 0
            ? badge(`安装时适配 ${platform.compatRewrites} 项`, "info")
            : null,
        ].filter(Boolean))
      );

      rows.push(
        el("div", { class: "stack", style: "margin-top:8px" }, [
          el("div", {}, [
            el("span", { class: "muted", text: "支持的 frontmatter 字段：" }),
            el("span", {
              class: "mono",
              text: platform.supportedFields.join(", ") || "（未声明）",
            }),
          ]),
          el("div", {}, [
            el("span", {
              class: "muted",
              text: platform.listingLimit
                ? `列表长度上限：${platform.listingLimit} 字符`
                : "列表长度上限：未声明",
            }),
          ]),
          el("div", {}, [
            el("span", {
              class: "muted",
              text: platform.supportsResources
                ? `资源目录：${platform.resourceDirs.join(", ") || "无"}`
                : "不支持附带资源目录",
            }),
          ]),
          platform.notes
            ? el("div", { class: "notice info", text: platform.notes })
            : null,
        ].filter(Boolean))
      );

      const targets = targetsFor(platform.id);
      if (targets.length === 0) {
        rows.push(
          el("div", {
            class: "muted",
            style: "margin-top:8px",
            text: "没有可用目标：可在「设置」中为该平台添加自定义路径。",
          })
        );
      } else {
        for (const target of targets) rows.push(targetRow(target));
      }

      append(refs.platformGrid, [el("div", { class: "card" }, rows)]);
    }
  }

  function targetRow(target) {
    const actions = [];
    const openSkill = state.snapshot && state.snapshot.editor;
    if (openSkill && openSkill.parseOk && openSkill.name) {
      actions.push(
        button("安装预览", "btn primary small", () =>
          // `preview_install` returns `InstallPreview`, not a `Snapshot`.
          call("preview_install", {
            platformId: target.platformId,
            scope: target.scope,
            path: target.path,
          }).then((preview) => preview && showInstallModal(preview))
        )
      );
    }
    if (target.installed) {
      actions.push(
        button("卸载", "btn danger small", () => askUninstall(target))
      );
    }
    actions.push(
      button("打开目录", "btn small", () => call("open_target", { path: target.path }))
    );

    return el("div", { class: "target-row" }, [
      badge(target.scopeLabel, "neutral"),
      target.installed ? badge("已安装", "installed") : null,
      el("span", { class: "target-path", text: target.path }),
      target.exists ? null : badge("目录不存在", "warning"),
      el("div", { class: "spacer" }),
    ].filter(Boolean).concat(actions));
  }

  function askUninstall(target) {
    const preview = state.snapshot.editor;
    const title = `卸载 “${preview ? preview.name : ""}”`;
    call("preview_uninstall", {
      platformId: target.platformId,
      scope: target.scope,
      path: target.path,
    }).then((plan) => {
      if (!plan) return;
      const body = [
        el("div", { text: `目标目录：${plan.targetRoot}` }),
        el("div", {
          class: "muted",
          text: `只会删除本应用写入的 ${plan.managedFiles} 个文件；其他文件保持不变。`,
        }),
      ];
      if (plan.hasExternalChanges) {
        body.push(
          el("div", {
            class: "notice warning",
            text: "检测到这些文件在安装之后被外部修改过，卸载仍会删除它们。",
          })
        );
      }
      askConfirm({
        title,
        message: "",
        nodes: body,
        confirmLabel: "卸载",
        onConfirm: () =>
          run("uninstall", {
            platformId: target.platformId,
            scope: target.scope,
            path: target.path,
            confirm: true,
          }),
      });
    });
  }

  // ---------------------------------------------------------------------
  // Install modal
  // ---------------------------------------------------------------------

  function showInstallModal(preview) {
    $("install-title").textContent = preview.title;
    const body = $("install-body");
    clear(body);

    append(body, [
      el("div", { class: "path-chip", text: preview.targetRoot }),
      el("div", { class: "summary-grid" }, [
        summaryBox("新增", preview.added),
        summaryBox("更新", preview.modified),
        summaryBox("删除", preview.removed),
        summaryBox("无变化", preview.unchanged),
      ]),
      preview.createsTarget
        ? el("div", { class: "notice info", text: "目标目录尚不存在，将新建。" })
        : null,
      preview.willBackup
        ? el("div", {
            class: "notice info",
            text: "目标已存在，覆盖前会先创建备份。",
          })
        : null,
      preview.hasExternalModification
        ? el("div", {
            class: "notice warning",
            text: "目标目录中的部分文件在本应用之外被修改过；继续安装会覆盖它们（已保留备份）。",
          })
        : null,
      preview.warnings && preview.warnings.length
        ? el("div", { class: "notice warning" }, [
            el("div", {
              text: `安装副本在该平台仍有 ${preview.warnings.length} 项兼容提醒（仍可继续安装）：`,
            }),
            ...preview.warnings.map((w) => el("div", { text: `• ${w.message}` })),
          ])
        : null,
      preview.rewrites && preview.rewrites.length
        ? el("div", { class: "notice info" }, [
            el("div", {
              text: "安装时会自动适配以下字段（只改安装副本，不改工作区源文件）：",
            }),
            ...preview.rewrites.map((r) =>
              el("div", { text: `• ${r.from} → ${r.to}：${r.reason}` })
            ),
          ])
        : null,
      diffTable(preview.entries),
    ].filter(Boolean));

    $("install-confirm").textContent =
      preview.warnings && preview.warnings.length ? "仍然安装" : "确认安装";
    $("modal-install").hidden = false;
    $("install-confirm").focus();
  }

  function summaryBox(label, value) {
    return el("div", { class: "summary-box" }, [
      el("div", { class: "k", text: label }),
      el("div", { class: "v", text: String(value) }),
    ]);
  }

  function diffTable(entries) {
    const rows = [
      el("tr", {}, [
        el("th", { text: "文件" }),
        el("th", { text: "变化" }),
        el("th", { text: "说明" }),
      ]),
    ];
    for (const entry of entries) {
      rows.push(
        el("tr", {}, [
          el("td", { text: entry.path }),
          el("td", { class: "change-" + entry.change, text: changeLabel(entry.change) }),
          el("td", { text: entry.detail || "" }),
        ])
      );
    }
    return el("table", { class: "diff" }, [el("tbody", {}, rows)]);
  }

  function changeLabel(change) {
    return (
      { added: "新增", modified: "更新", unchanged: "无变化", removed: "删除" }[
        change
      ] || change
    );
  }

  function hideInstallModal() {
    $("modal-install").hidden = true;
  }

  // ---------------------------------------------------------------------
  // Generic confirmation modal
  // ---------------------------------------------------------------------

  function askConfirm(options) {
    $("confirm-title").textContent = options.title;
    const body = $("confirm-body");
    clear(body);
    if (options.message) {
      append(body, [
        el("div", { style: "white-space:pre-wrap", text: options.message }),
      ]);
    }
    for (const node of options.nodes || []) append(body, [node]);
    state.confirmAction = options.onConfirm;
    $("confirm-ok").textContent = options.confirmLabel || "确定";
    $("modal-confirm").hidden = false;
    $("confirm-cancel").focus();
  }

  function hideConfirmModal() {
    $("modal-confirm").hidden = true;
    state.confirmAction = null;
  }

  // ---------------------------------------------------------------------
  // Frontmatter guide (read-only)
  // ---------------------------------------------------------------------

  async function openGuide() {
    const guide = await ensureGuide();
    if (!guide) return;
    state.guideTab = "required";
    state.guideQuery = "";
    if (refs.guideSearch) refs.guideSearch.value = "";
    renderGuide();
    $("modal-guide").hidden = false;
    if (refs.guideSearch) refs.guideSearch.focus();
  }

  function hideGuideModal() {
    $("modal-guide").hidden = true;
  }

  function renderGuide() {
    const guide = state.guide;
    if (!guide || !refs.guideBody) return;

    clear(refs.guideTabs);
    for (const tab of GUIDE_TABS) {
      refs.guideTabs.appendChild(
        button(
          tab.label,
          "tab" + (state.guideTab === tab.id ? " active" : ""),
          () => {
            state.guideTab = tab.id;
            state.guideQuery = "";
            if (refs.guideSearch) refs.guideSearch.value = "";
            renderGuide();
          }
        )
      );
    }

    const query = state.guideQuery.trim().toLowerCase();
    renderGuideStatus(guide, !query && state.guideTab === "required");
    clear(refs.guideBody);

    if (query) {
      const fields = guide.fields.filter((field) => fieldMatches(field, query));
      const rules = guide.rules.filter((rule) => ruleMatches(rule, query));
      const body = [...guide.directoryRules, ...guide.bodyRules].filter((item) =>
        item.toLowerCase().includes(query)
      );
      if (fields.length === 0 && rules.length === 0 && body.length === 0) {
        append(refs.guideBody, [
          el("div", { class: "empty", text: "没有匹配的字段或规则。" }),
        ]);
        return;
      }
      append(refs.guideBody, [
        fields.length
          ? el("div", { class: "guide-section" }, [
              el("h3", { class: "section-title", text: `字段（${fields.length}）` }),
              ...fields.map((field) => guideFieldCard(field, guide)),
            ])
          : null,
        body.length
          ? el("div", { class: "guide-section" }, [
              el("h3", { class: "section-title", text: "正文与目录规则" }),
              ruleList(body),
            ])
          : null,
        rules.length
          ? el("div", { class: "guide-section" }, [
              el("h3", { class: "section-title", text: `校验规则（${rules.length}）` }),
              ...rules.map(guideRuleCard),
            ])
          : null,
      ]);
      return;
    }

    switch (state.guideTab) {
      case "optional":
        append(refs.guideBody, [
          el("div", { class: "guide-section" }, [
            el("h3", { class: "section-title", text: "规范可选字段" }),
            ...guide.fields
              .filter((field) => field.category === "spec-optional")
              .map((field) => guideFieldCard(field, guide)),
          ]),
        ]);
        break;
      case "platform":
        append(refs.guideBody, [
          guidePlatformSection(guide),
          el("div", { class: "guide-section" }, [
            el("h3", { class: "section-title", text: "平台扩展字段" }),
            ...guide.fields
              .filter((field) => field.category === "platform")
              .map((field) => guideFieldCard(field, guide)),
          ]),
        ]);
        break;
      case "body":
        append(refs.guideBody, [
          el("div", { class: "guide-section" }, [
            el("h3", { class: "section-title", text: "目录规则" }),
            ruleList(guide.directoryRules),
          ]),
          el("div", { class: "guide-section" }, [
            el("h3", { class: "section-title", text: "正文规则" }),
            ruleList(guide.bodyRules),
          ]),
          el("div", { class: "guide-section" }, [
            el("h3", { class: "section-title", text: "完整 SKILL.md 示例" }),
            el("pre", { class: "guide-code guide-example" }, [text(guide.example)]),
          ]),
        ]);
        break;
      case "rules":
        append(refs.guideBody, [
          el("div", { class: "guide-section" }, [
            el("h3", { class: "section-title", text: "校验规则" }),
            el("p", {
              class: "muted",
              text: "`<ID>` 会替换为实际平台 id，例如 PLAT-CLAUDE-CODE-FIELD。推荐章节只报建议，不是规范错误。",
            }),
            ...guide.rules.map(guideRuleCard),
          ]),
        ]);
        break;
      default:
        append(refs.guideBody, [
          el("div", { class: "guide-section" }, [
            el("h3", { class: "section-title", text: "必填字段" }),
            ...guide.fields
              .filter((field) => field.category === "required")
              .map((field) => guideFieldCard(field, guide)),
          ]),
        ]);
        break;
    }
  }

  function renderGuideStatus(guide, visible) {
    const editor = state.snapshot && state.snapshot.editor;
    clear(refs.guideStatus);
    refs.guideStatus.hidden = !visible;
    if (!visible) return;
    if (!editor) {
      append(refs.guideStatus, [
        el("span", { class: "muted", text: "当前文档：尚未打开 Skill" }),
      ]);
      return;
    }
    if (!editor.parseOk) {
      append(refs.guideStatus, [
        el("div", { class: "guide-status-row" }, [
          el("span", { class: "muted", text: "当前文档：" }),
          badge("frontmatter 无法解析", "error"),
          el("span", {
            class: "muted",
            text: "修复 YAML 后可查看字段状态",
          }),
        ]),
      ]);
      return;
    }

    const keys = new Set(editor.frontmatterKeys);
    const required = guide.fields.filter((field) => field.required);
    const used = guide.fields.filter((field) => keys.has(field.key));
    const missing = required.filter((field) => !keys.has(field.key));
    const descriptionCount = Array.from(currentDescriptionValue()).length;

    append(refs.guideStatus, [
      el("div", { class: "guide-status-row" }, [
        el("span", { class: "muted", text: "当前文档：" }),
        badge(
          missing.length ? "缺少必填" : "必填完整",
          missing.length ? "warning" : "ok"
        ),
        el("span", { text: `已用 ${used.length} 个字段` }),
        el("span", { class: "muted", text: "·" }),
        el("span", {
          text: `description ${descriptionCount} / ${guide.limits.descriptionMax}`,
        }),
        missing.length
          ? el("span", { class: "muted", text: "·" })
          : null,
        missing.length
          ? el("span", {
              class: "muted",
              text: `缺少 ${missing.map((field) => field.key).join("、")}`,
            })
          : null,
      ].filter(Boolean)),
    ]);
  }

  function fieldMatches(field, query) {
    return [
      field.key,
      field.label,
      field.valueType,
      field.summary,
      field.example,
      field.commonMistake,
      ...field.rules,
      ...field.aliases,
    ]
      .join(" ")
      .toLowerCase()
      .includes(query);
  }

  function ruleMatches(rule, query) {
    return [
      rule.id,
      rule.sourceLabel,
      rule.summary,
      rule.fix,
      rule.severity,
    ]
      .join(" ")
      .toLowerCase()
      .includes(query);
  }

  function guideFieldCard(field, guide) {
    return el("div", { class: "guide-field" }, [
      el("div", { class: "guide-field-head" }, [
        el("code", { class: "guide-key", text: field.key }),
        el("span", { class: "guide-field-label", text: field.label }),
        badge(field.required ? "必填" : "可选", field.required ? "error" : "neutral"),
        el("span", { class: "muted", text: field.valueType }),
        el("div", { class: "spacer" }),
        ...guidePlatformChips(field, guide),
      ]),
      el("div", { class: "guide-summary", text: field.summary }),
      ruleList(field.rules),
      el("div", { class: "guide-example-label", text: "YAML 示例" }),
      el("pre", { class: "guide-code" }, [text(field.example)]),
      field.commonMistake
        ? el("div", { class: "guide-mistake" }, [
            badge("常见错误", "warning"),
            el("span", { text: field.commonMistake }),
          ])
        : null,
      field.aliases && field.aliases.length
        ? el("div", {
            class: "muted",
            text: `相关字段：${field.aliases.join("、")}`,
          })
        : null,
      field.docUrl ? docLink(field.docUrl) : null,
    ].filter(Boolean));
  }

  function guidePlatformChips(field, guide) {
    if (field.platformIds.length === 0) {
      return [el("span", { class: "muted", text: "无平台声明" })];
    }
    if (field.platformIds.length === guide.platforms.length) {
      return [badge("所有平台", "info")];
    }
    return field.platformIds.map((id) => {
      const platform = guide.platforms.find((item) => item.id === id);
      return el("span", { class: "chip", text: platform ? platform.name : id });
    });
  }

  function guidePlatformSection(guide) {
    return el("div", { class: "guide-section" }, [
      el("h3", { class: "section-title", text: "平台支持与列表上限" }),
      el("div", { class: "guide-platforms" }, guide.platforms.map((platform) =>
        el("div", { class: "guide-platform" }, [
          el("div", { class: "guide-platform-head" }, [
            el("b", { text: platform.name }),
            el("div", { class: "spacer" }),
            platform.listingLimit
              ? badge(`列表上限 ${platform.listingLimit}`, "info")
              : badge("未声明列表上限", "neutral"),
          ]),
          el("div", { class: "muted", text: platform.notes }),
          platform.docsUrl ? docLink(platform.docsUrl) : null,
        ].filter(Boolean))
      )),
    ]);
  }

  function guideRuleCard(rule) {
    return el("div", { class: "guide-rule" }, [
      el("div", { class: "guide-rule-head" }, [
        badge(severityLabel(rule.severity), rule.severity),
        el("code", { class: "guide-key", text: rule.id }),
        el("span", { class: "muted", text: rule.sourceLabel }),
        el("div", { class: "spacer" }),
        rule.docUrl ? docLink(rule.docUrl) : null,
      ].filter(Boolean)),
      el("div", { class: "guide-summary", text: rule.summary }),
      el("div", { class: "guide-fix", text: `修复：${rule.fix}` }),
    ]);
  }

  function ruleList(items) {
    return el("ul", { class: "guide-rules" }, items.map((item) =>
      el("li", { text: item })
    ));
  }

  function docLink(url) {
    return button("查看官方文档", "btn link", () => call("open_url", { url }));
  }

  // ---------------------------------------------------------------------
  // Settings page
  // ---------------------------------------------------------------------

  function buildSettingsShell() {
    const host = $("page-settings");
    clear(host);

    refs.settingsWorkspace = el("input", { type: "text", readonly: true });
    refs.settingsTheme = el("select", {
      onchange: (event) =>
        run("update_settings", { patch: { theme: event.target.value } }),
    });
    append(refs.settingsTheme, [
      el("option", { value: "system", text: "跟随系统" }),
      el("option", { value: "light", text: "浅色" }),
      el("option", { value: "dark", text: "深色" }),
    ]);
    refs.settingsBackups = el("input", {
      type: "number",
      min: "1",
      max: "50",
      onchange: (event) =>
        run("update_settings", {
          patch: { backupCount: Number(event.target.value) || 1 },
        }),
    });
    refs.settingsDetect = el("input", {
      type: "checkbox",
      onchange: (event) =>
        run("update_settings", {
          patch: { detectExternalChanges: event.target.checked },
        }),
    });
    refs.settingsPaths = el("div", { class: "stack" });

    append(host, [
      el("div", { class: "card" }, [
        el("div", { class: "card-head" }, [
          el("div", { class: "spacer" }, [
            el("div", { class: "card-title", text: "通用" }),
            el("div", {
              class: "card-sub",
              text: "设置保存在本地 JSON 文件，不含 Skill 正文、密钥或任何遥测数据。",
            }),
          ]),
          button("保存设置", "btn primary", () => run("save_settings")),
        ]),
        el("div", { class: "stack" }, [
          field(
            "默认工作区",
            el("div", { class: "row" }, [
              refs.settingsWorkspace,
              button("浏览…", "btn", async () => {
                await run("choose_workspace");
                renderSettings();
              }),
            ])
          ),
          field("主题", refs.settingsTheme),
          field("每个 Skill 保留的备份数量（1–50）", refs.settingsBackups),
          el("div", { class: "checkbox" }, [
            refs.settingsDetect,
            el("label", {
              text: "安装前检测目标文件是否被外部修改",
            }),
          ]),
        ]),
      ]),
      el("div", { class: "card" }, [
        el("div", { class: "card-head" }, [
          el("div", { class: "spacer" }, [
            el("div", { class: "card-title", text: "本地路径" }),
            el("div", {
              class: "card-sub",
              text: "所有内容都保存在本机；应用不会上传任何数据。",
            }),
          ]),
          button("打开配置目录", "btn", () => call("open_config_dir")),
        ]),
        refs.settingsPaths,
      ]),
    ]);
  }

  function renderSettings() {
    const snapshot = state.snapshot;
    if (!snapshot || !refs.settingsTheme) return;
    const settings = snapshot.settings;

    if (document.activeElement !== refs.settingsWorkspace)
      refs.settingsWorkspace.value = settings.workspace;
    refs.settingsTheme.value = settings.theme;
    if (document.activeElement !== refs.settingsBackups)
      refs.settingsBackups.value = String(settings.backupCount);
    refs.settingsDetect.checked = settings.detectExternalChanges;

    clear(refs.settingsPaths);
    const rows = [
      ["配置文件", settings.configPath],
      ["工作区", settings.workspace],
      ["安装清单", settings.manifestDir],
      ["备份目录", settings.backupDir],
      ["日志目录", settings.logDir],
    ];
    for (const [label, value] of rows) {
      append(refs.settingsPaths, [
        el("div", {}, [
          el("div", { class: "muted", text: label }),
          el("div", { class: "path-chip", text: value }),
        ]),
      ]);
    }
  }

  // ---------------------------------------------------------------------
  // Wiring
  // ---------------------------------------------------------------------

  function wireChrome() {
    refs.guideSearch = $("guide-search");
    refs.guideTabs = $("guide-tabs");
    refs.guideStatus = $("guide-status");
    refs.guideBody = $("guide-body");

    for (const item of document.querySelectorAll(".nav-item")) {
      item.addEventListener("click", () => setPage(item.dataset.page));
    }
    $("btn-refresh").addEventListener("click", async () => {
      await flushEdits();
      await run("refresh_skills");
    });
    refs.guideSearch.addEventListener("input", (event) => {
      state.guideQuery = event.target.value;
      renderGuide();
    });
    $("guide-close").addEventListener("click", hideGuideModal);
    $("guide-done").addEventListener("click", hideGuideModal);
    $("modal-guide").addEventListener("click", (event) => {
      if (event.target === $("modal-guide")) hideGuideModal();
    });
    $("install-cancel").addEventListener("click", async () => {
      hideInstallModal();
      await run("cancel_install");
    });
    $("install-confirm").addEventListener("click", async () => {
      hideInstallModal();
      await run("confirm_install");
    });
    $("modal-install").addEventListener("click", (event) => {
      if (event.target === $("modal-install")) $("install-cancel").click();
    });
    $("confirm-cancel").addEventListener("click", () => {
      hideConfirmModal();
    });
    $("confirm-ok").addEventListener("click", async () => {
      const action = state.confirmAction;
      hideConfirmModal();
      if (action) await action();
    });
    $("modal-confirm").addEventListener("click", (event) => {
      if (event.target === $("modal-confirm")) $("confirm-cancel").click();
    });

    document.addEventListener("keydown", (event) => {
      if (event.key !== "Escape") return;
      if (!$("modal-guide").hidden) hideGuideModal();
      else if (!$("modal-install").hidden) $("install-cancel").click();
      else if (!$("modal-confirm").hidden) $("confirm-cancel").click();
    });
  }

  function wireCloseGuard() {
    if (!listen) return;
    listen("skillforge://unsaved-edits", () => {
      showStatus(
        "warning",
        "有未保存的修改：请先「保存」，然后再次关闭窗口。"
      );
    });
  }

  async function main() {
    buildSkillsShell();
    buildEditorShell();
    buildPlatformsShell();
    buildSettingsShell();
    wireChrome();
    wireCloseGuard();

    const snapshot = await call("boot");
    if (snapshot) {
      applySnapshot(snapshot);
      log(
        "info",
        `已就绪：${snapshot.skills.length} 个 Skill，工作区 ${snapshot.workspace}`
      );
    } else {
      log("error", "启动失败：无法获取初始状态（boot 命令失败）");
    }
    renderNav();
  }

  window.addEventListener("error", (event) => {
    const where = event.filename ? `${event.filename}:${event.lineno}` : "";
    const message = `${event.message || "未知错误"} ${where}`;
    showStatus("error", `前端错误：${message}`);
    log("error", message);
  });

  window.addEventListener("unhandledrejection", (event) => {
    const reason = event.reason && event.reason.message ? event.reason.message : String(event.reason);
    showStatus("error", `未处理的异步错误：${reason}`);
    log("error", `未处理的异步错误：${reason}`);
  });

  main();
})();
