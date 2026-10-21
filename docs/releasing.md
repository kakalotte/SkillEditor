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

## 1.1 签名开关

是否签名由 `SIGN_BUILDS` 决定，规则如下：

| 触发方式 | 是否签名 |
| --- | --- |
| 推送 `v*` tag | **始终签名**，证书缺失时构建失败 |
| 手动 `workflow_dispatch`，`sign=false`（默认） | 不签名 |
| 手动 `workflow_dispatch`，`sign=true` | 签名，证书缺失时构建失败 |

这样设计的目的是：**tag 推送永远不敢静默产出未签名包**，而在证书还没
买好的阶段，仍然可以手动触发一次未签名发布。未签名发布时，Release
说明里会写入绕过 Gatekeeper / SmartScreen 的提示。

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

### 2.1 证书怎么获得

#### Apple Developer ID（macOS）

1. 加入 **Apple Developer Program**（个人或组织，99 USD/年）。
   免费 Apple ID 不能做 Developer ID 签名，也不能公证。
2. 在 Mac 的「钥匙串访问」→「证书助理」→「从证书颁发机构请求证书」，
   生成 CSR 并保存到磁盘。
3. 登录 <https://developer.apple.com/account/resources/certificates>，
   新建 **Developer ID Application** 证书，上传 CSR，下载 `.cer`。
4. 双击 `.cer` 安装到登录钥匙串，确认能看到签名身份：

   ```bash
   security find-identity -v -p codesigning
   # 应出现 Developer ID Application: Your Name (TEAMID)
   ```

5. 在「钥匙串访问」中导出证书和私钥为 `.p12`，设置导出密码。
   这个 `.p12` 和密码就是 `APPLE_CERTIFICATE` 与
   `APPLE_CERTIFICATE_PASSWORD`。
6. 在 <https://account.apple.com> 的「登录与安全」中创建 **App 专用密码**，
   用于 `APPLE_PASSWORD`；`APPLE_TEAM_ID` 在开发者账号 Membership 页面查看。

组织账号需要 D-U-N-S 编号，审核时间可能比个人账号长。

#### Windows 代码签名

当前发布工作流按 **PFX 导入**设计，适合私钥可导出的证书。
2023 年之后，CA 新签发的代码签名证书通常要求私钥存放在硬件令牌或 HSM 中，
不能直接导出 PFX，因此实际有两条路线：

1. **Azure Trusted Signing（推荐用于 CI）**
   - 需要 Azure 订阅，完成组织/个人身份验证；
   - 由 Microsoft 托管签名，GitHub Actions 可直接调用；
   - 费用通常低于传统 EV 证书，且不需要把私钥放进仓库 Secrets；
   - 采用这条路线时，需要把当前 workflow 的 PFX 导入步骤替换为
     Azure Trusted Signing 的 action 或 `signCommand`。
2. **传统 OV / EV 代码签名证书**
   - DigiCert、Sectigo、GlobalSign、SSL.com 等 CA 均有提供；
   - OV 通常约 200–400 USD/年，EV 通常更贵；
   - 购买前必须确认 CA 是否提供 CI 可用的 PFX 或云签名服务；
     只在硬件令牌里、无法导出的证书不能直接用于当前 workflow；
   - EV 证书对 SmartScreen 信誉更有利，但新证书仍可能需要时间积累声誉。

3. **自签名证书**
   - 免费，但 Windows 不信任，只适合内部测试；
   - 不能作为公开发布包的签名方案。

如果暂时没有 Windows 证书，可以先推送代码并跑三平台 CI，
但不要打发布 tag；否则发布工作流会按设计直接失败。

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

tag 推送会**强制签名**，没有证书时构建直接失败。

也可以手动触发（tag 必须已经存在）：

```bash
gh workflow run release.yml -f tag=v0.1.0
```

#### 未签名发布（证书尚未购买时）

手动触发时把 `sign` 关掉即可，这是默认值：

```bash
gh workflow run release.yml -f tag=v0.1.0 -f sign=false
```

仓库没有安装 `gh` CLI 时，也可以在 GitHub 网页端
**Actions → Release → Run workflow** 里填写 tag 并取消勾选 `sign`。

未签名产物的后果：

- macOS 是 ad-hoc 签名，用户首次打开需右键选择「打开」；
- Windows 安装包会触发 SmartScreen 的「更多信息 → 仍要运行」；
- Linux 本来就未签名，没有额外影响。

拿到证书并配好 Secrets 之后，改回推送 tag 的方式即可获得签名产物。

### 3.3 验收 Draft Release

在 GitHub 的 Releases 页面下载所有产物并检查：

- macOS arm64 与 x86_64 各一个 `.dmg`；
- Windows 有 NSIS `.exe` 与 `.msi`；
- Linux 有 `.deb` 与 `.AppImage`；
- 每个安装包都能启动，且能完成一次打开工作区、编辑、保存、导入/导出；
- 签名构建：macOS 的 `.app` 通过 Gatekeeper，Windows 安装包带有效签名；
- 未签名构建：确认 Release 说明里已写明绕过 Gatekeeper / SmartScreen 的办法；
- Linux AppImage 在已安装 FUSE 的桌面环境可以启动，否则用
  `--appimage-extract-and-run` 验收。

签名构建的本地检查示例：

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
- Apple 公证需要有效的 Apple Developer Program 账号和 Developer ID 证书；
  在购买之前只能发布未签名版本（见 1.1 与 3.2）。
- Windows 证书签名不会立刻消除 SmartScreen 的声誉提示。
- Linux `.AppImage` 依赖 FUSE；README 需要保留
  `--appimage-extract-and-run` 的备用说明。
- 发布工作流只负责构建与上传，不自动发布 Draft Release。
- 未签名发布只是过渡方案；面向公众分发前应补上 macOS 与 Windows 签名。
