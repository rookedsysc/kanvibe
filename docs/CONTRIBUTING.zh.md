# KanVibe 贡献指南

[EN](../CONTRIBUTING.md) | [KO](./CONTRIBUTING.ko.md)

感谢您对 KanVibe 的关注！本指南将帮助您开始贡献。

---

## 开发环境设置

### 前置要求

- Rust stable 及 `rustfmt`、`clippy`（edition 2024）
- [cargo-nextest](https://nexte.st/)
- Herdr、tmux 或 zellij（用于终端功能）
- 仅 Linux：GPUI 与 WebKitGTK 开发包，例如 Ubuntu：
  `pkg-config libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev libwebkit2gtk-4.1-dev`

### 开始

```bash
# 克隆仓库
git clone https://github.com/rookedsysc/kanvibe.git
cd kanvibe

# 从源码启动桌面应用（debug 构建，开发数据库）
cargo run -p kanvibe-app
```

debug 构建使用开发数据目录和 hook 端口 `19736`。参见 [开发与 QA 数据库](./README.zh.md)。

### 发布构建

```bash
# Linux：在 dist/ 生成 tarball 和 .deb（--verify 解包后启动一次进行确认，
# --allow-missing-agents 跳过本机无法构建的远程 agent 目标）
packaging/linux/build-linux.sh --verify

# macOS：签名并公证的 KanVibe.app 与 dist/KanVibe-${version}.dmg
packaging/macos/build-macos.sh
```

两个脚本都以 `packaged` cargo feature 构建 `kanvibe-app`。只有该二进制使用已安装应用的数据目录和 hook 端口 `9736`；不带该 feature 的 `cargo build --release` 以手动 QA 方式运行（`<appData>/kanvibe-qa`，端口 `29736`）。

版本号取自根目录 `Cargo.toml` 的 `workspace.package.version`。`build-macos.sh` 从环境变量或 `.env.example` 中列出的键读取签名凭据；`--unsigned` 可在没有凭据时进行本地构建检查。

- macOS 输出：`dist/KanVibe-${version}.dmg`
- Homebrew cask：更新 `Casks/kanvibe.rb`

---

## Pull Request 指南

### 提交前检查

1. **所有检查必须通过：**
   ```bash
   cargo fmt --all --check                                  # 格式
   cargo clippy --workspace --all-targets -- -D warnings    # Lint
   cargo nextest run --workspace                            # 测试
   cargo xtask parity check                                 # TS 等价性台账
   cargo build --workspace --release                        # 发布构建
   ```

2. **附上工作截图或 GIF** 展示您的更改。没有功能运行视觉证据的 PR 将不会被合并。

3. **遵循现有代码风格。** 项目使用：
   - `crates/` 下的 Rust workspace（按层划分的 crate：`kanvibe-domain`、`kanvibe-db`、`kanvibe-services`、`kanvibe-app` 等）
   - 使用语义主题令牌（`crates/kanvibe-app/src/theme.rs`）的 GPUI 视图
   - 所有面向用户的字符串放在 `messages/*.json`
   - 通过 `rusqlite` 访问的内置 SQLite（`kanvibe-db`）

### PR 流程

1. Fork 仓库
2. 从 `main` 创建功能分支
3. 进行更改
4. 运行所有检查（见上文）
5. 使用 [Conventional Commits](https://www.conventionalcommits.org/) 格式提交
6. Push 并创建 Pull Request
7. 附上功能运行的截图/GIF

### 提交消息格式

```
feat(scope): 添加新功能
fix(scope): 修复特定错误
docs: 更新文档
refactor(scope): 代码重构
```

---

## 国际化（i18n）

所有面向用户的字符串都需要翻译。添加或修改 UI 文本时：

1. 在 `messages/ko.json` 中添加键/值
2. 在 `messages/en.json` 和 `messages/zh.json` 中添加翻译
3. 在视图中通过 `kanvibe_domain::i18n`（`Translator::translate` 或 `translate`）读取

---

## 数据库更改

KanVibe 使用内置 SQLite 数据库（`crates/kanvibe-db`）。修改表结构时：

1. 在 `crates/kanvibe-db/src/migrations/mod.rs` 的 `MIGRATIONS` 中添加迁移
2. 更新 `crates/kanvibe-db/src/schema/` 中的幂等 schema 修复，使旧数据库也达到相同的 schema

---

## 文档更新

当进行影响用户体验的更改时，请同时更新相应文档：

| 更改内容 | 需要更新 |
|----------|----------|
| 功能或 UI | `README.md`、`docs/README.ko.md`、`docs/README.zh.md` |
| 贡献流程 | `docs/CONTRIBUTING.md`、`docs/CONTRIBUTING.ko.md`、`docs/CONTRIBUTING.zh.md` |
| 环境变量 | `.env.example` + 所有 README 文件 |
| Hook API | 所有 README 文件（Hook API 部分） |

三种语言版本（EN、KO、ZH）必须同时更新。

---

## 下一步 & 路线图

### Gemini Hooks / Codex Hooks 支持（开发中）

目前，KanVibe 支持 **Claude Code Hooks** 进行自动状态追踪。**Gemini Hooks** 和 **Codex Hooks** 支持已计划并正在开发中。

如果您对多代理 hook 支持有更好的方向或架构想法，请先开启 [Discussion](https://github.com/rookedsysc/kanvibe/discussions)。我们希望在实现之前共同找到最佳方向。

### 令牌使用量仪表板

下一个主要功能目标是**令牌使用量追踪仪表板** - 监控各任务和会话的 AI 代理令牌消耗。

### 如何提出新功能

对于重大更改或新功能：
1. 在 [Discussion](https://github.com/rookedsysc/kanvibe/discussions) 中分享您的想法
2. 获取社区和维护者的反馈
3. 方向达成一致后，创建 Issue 并提交 PR

---

## 许可证

通过为 KanVibe 做出贡献，您同意您的贡献将根据 [AGPL-3.0 许可证](../LICENSE) 进行许可。
