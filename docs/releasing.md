# 发布与签名

本文说明 SkillForge 如何通过 GitHub Actions 生成已签名的 macOS / Windows
安装包，以及未签名的 Linux 包。日常开发仍然只需要 Rust 工具链：Tauri CLI
只在 CI 发布任务中使用。

- 仓库：<https://github.com/kakalotte/SkillEditor>
- CI：`.github/workflows/ci.yml`
- 发布：`.github/workflows/release.yml`
- 当前版本：`0.1.0`（`Cargo.toml` 与 `src-tauri/tauri.conf.json` 必须一致）

## 1. 发布矩阵

| 目标 | Runner | Tauri 参数 | 产物 |
| --- | --- | --- | --- |
| macOS arm64 | `macos-latest` | `--target aarch64-apple-darwin --bundles dmg` | `.dmg` |
| macOS x86_64 | `macos-latest` | `--target x86_64-apple-darwin --bundles dmg` | `.dmg` |
| Windows x64 | `windows-latest` | `--bundles nsis,msi` | `setup.exe`、`.msi` |
| Linux x64 | `ubuntu-22.04` | `--bundles deb,appimage` | `.deb`、`.AppImage` |

发布工作流由 `v*` tag 触发，先生成 **Draft Release**。人工下载并验收后，
再到 GitHub 上发布。四个任务都上传到同一个 Draft Release。

## 2. 必需的 GitHub Secrets

签名不是可选项：发布工作流在构建前会检查 Secrets，缺失时直接失败，
避免静默产出未签名安装包。

### Apple（macOS 签名 + 公证）

| Secret | 内容 |
| --- | --- |
| `APPLE_CERTIFICATE` | Developer ID Application `.p12` 的 base64 |
| `APPLE_CERTIFICATE_PASSWORD` | `.p12` 导出密码 |
| `APPLE_SIGNING_IDENTITY` | 例如 `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID` | Apple Developer 账号邮箱 |
| `APPLE_PASSWORD` | App 专用密码（不是账号密码） |
| `APPLE_TEAM_ID` | 10 位 Team ID |

检查本机可用的签名身份：

```bash
security find-identity -v -p codesigning
```

导出 `.p12` 后生成 base64（macOS）：

```bash
base64 -i DeveloperIDApplication.p12 -o apple-cert.p12.b64
```

### Windows（Authenticode 签名）

| Secret | 内容 |
| --- | --- |
| `WINDOWS_CERTIFICATE` | 代码签名 `.pfx` 的 base64 |
| `WINDOWS_CERTIFICATE_PASSWORD` | `.pfx` 密码 |
| `WINDOWS_TIMESTAMP_URL` | 可选；默认 `http://timestamp.digicert.com` |

PowerShell 生成 base64：

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("codesign.pfx")) |
  Set-Content -NoNewline codesign.pfx.b64
```

发布工作流会：

1. 把 `.pfx` 导入 `Cert:\CurrentUser\My`；
2. 从导入的证书读取 Thumbprint；
3. 生成只存在于 runner 上的 `src-tauri/tauri.windows.conf.json`；
4. 让 Tauri 用该证书签名 NSIS 与 MSI。

该覆盖文件已加入 `.gitignore`，不会提交。

### 写入 Secrets

如果使用 GitHub CLI：

```bash
gh secret set APPLE_CERTIFICATE < apple-cert.p12.b64
gh secret set APPLE_CERTIFICATE_PASSWORD --body 'p12-password'
gh secret set APPLE_SIGNING_IDENTITY --body 'Developer ID Application: Your Name (TEAMID)'
gh secret set APPLE_ID --body 'you@example.com'
gh secret set APPLE_PASSWORD --body 'app-specific-password'
gh secret set APPLE_TEAM_ID --body 'TEAMID'

gh secret set WINDOWS_CERTIFICATE < codesign.pfx.b64
gh secret set WINDOWS_CERTIFICATE_PASSWORD --body 'pfx-password'
gh secret set WINDOWS_TIMESTAMP_URL --body 'http://timestamp.digicert.com'
```

`GITHUB_TOKEN` 由 Actions 自动提供，不需要手动创建。

你也可以在仓库的 **Settings → Secrets and variables → Actions** 中添加。

## 3. 发布步骤

### 3.1 更新版本

同时修改以下两处，`frontend_contract` 测试会检查它们是否一致：

- `Cargo.toml` 的 `[workspace.package].version`
- `src-tauri/tauri.conf.json` 的 `version`

接着更新 `CHANGELOG.md`，运行本地质量门禁：

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
node --check ui/app.js
```

### 3.2 推送并打 tag

```bash
git add -A
git commit -m "Release v0.1.0"
git push origin main

git tag -a v0.1.0 -m "SkillForge v0.1.0"
git push origin v0.1.0
```

也可以手动触发（tag 必须已经存在）：

```bash
gh workflow run release.yml -f tag=v0.1.0
```

### 3.3 验收 Draft Release

在 GitHub 的 Releases 页面下载所有产物并检查：

- macOS arm64 与 x86_64 各一个 `.dmg`；
- Windows 有 NSIS `.exe` 与 `.msi`；
- Linux 有 `.deb` 与 `.AppImage`；
- 每个安装包都能启动，且能完成一次打开工作区、编辑、保存、导入/导出；
- macOS 的 `.app` 通过 Gatekeeper；Windows 安装包带有效签名；
- Linux AppImage 在已安装 FUSE 的桌面环境可以启动，否则用
  `--appimage-extract-and-run` 验收。

macOS 本地检查示例：

```bash
codesign --verify --deep --strict --verbose=2 /Applications/SkillForge.app
spctl -a -vvv -t exec /Applications/SkillForge.app
spctl -a -vvv -t open --context context:primary-signature -v SkillForge_*.dmg
xcrun stapler validate SkillForge_*.dmg
```

Windows 本地检查：

```powershell
signtool verify /pa /v SkillForge_*-setup.exe
signtool verify /pa /v SkillForge_*.msi
```

确认无误后，在 GitHub 上把 Draft Release 发布。发布后不要移动或删除 tag。

### 3.4 失败重试

如果某个矩阵任务失败，先修复代码或 Secrets，再删除并重新创建 tag：

```bash
git tag -d v0.1.0
git push origin :refs/tags/v0.1.0
git tag -a v0.1.0 -m "SkillForge v0.1.0"
git push origin v0.1.0
```

如果只是重新运行失败的任务，可以在 Actions 页面使用 **Re-run failed jobs**。
Draft Release 中已经上传的同类产物可能需要手动删除，避免重复。

## 4. 首次发布前仍需人工确认

- Windows/Linux 从未在开发机上构建过；第一次 CI 运行可能暴露平台差异。
- Apple 公证需要有效的 Apple Developer Program 账号和 Developer ID 证书。
- Windows 证书签名不会立刻消除 SmartScreen 的声誉提示。
- Linux `.AppImage` 依赖 FUSE；README 需要保留
  `--appimage-extract-and-run` 的备用说明。
- 发布工作流只负责构建与上传，不自动发布 Draft Release。
