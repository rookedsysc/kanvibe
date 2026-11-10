# KanVibe 贡献指南

[EN](../CONTRIBUTING.md) | [KO](./CONTRIBUTING.ko.md)

感谢您对 KanVibe 的关注！本指南将帮助您开始贡献。

---

## 开发环境设置

### 前置要求

- Node.js 24.x（参见 `.nvmrc`）
- Rust 1.90 或更新版本、Cargo 和 [Tauri 平台前置要求](https://v2.tauri.app/start/prerequisites/)
- pnpm
- Herdr、tmux 或 zellij（用于终端功能）

### 开始

```bash
# 克隆仓库
git clone https://github.com/rookedsysc/kanvibe.git
cd kanvibe

# 安装依赖
pnpm install

# 以开发模式启动 Tauri 桌面应用
pnpm dev
```

运行 `pnpm dev` 会打开原生 Tauri 桌面窗口，并启动用于 React 渲染器的 Vite。SQLite 会在应用启动时自动初始化。

可选运行时设置请参阅 [.env.example](../.env.example)。请将变量显式传给桌面应用进程；应用不会自动加载 `.env`。

### 发布构建

```bash
pnpm dist
```

`pnpm dist` 会构建 React 渲染器和 Rust 后端，并生成 Tauri 桌面安装包。

- 桌面安装包：`src-tauri/target/release/bundle/`
- macOS DMG：`src-tauri/target/release/bundle/dmg/`
- Homebrew cask：更新 `Casks/kanvibe.rb`

---

## Pull Request 指南

### 提交前检查

1. **所有检查必须通过：**
   ```bash
   pnpm build    # 构建成功
   pnpm check    # 类型检查通过
   pnpm test     # React 行为测试通过
   pnpm test:rust # Rust 服务和持久化测试通过
   ```

2. **附上工作截图或 GIF** 展示您的更改。没有功能运行视觉证据的 PR 将不会被合并。

3. **遵循现有代码风格。** 项目使用：
   - TypeScript 严格模式
   - Tailwind CSS v4 + 设计令牌（CSS 变量）
   - next-intl 处理所有面向用户的字符串
   - 内置 SQLite + rusqlite 数据访问

### PR 流程

1. Fork 仓库
2. 从 `main` 创建功能分支
3. 进行更改
4. 运行所有检查（`pnpm build && pnpm check && pnpm test && pnpm test:rust`）
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
3. 在组件中通过 `useTranslations` 或 `getTranslations` 使用 `t("key")`

---

## 数据库更改

原生应用启动时，会通过 `src-tauri/src/database.rs` 初始化 SQLite 并应用兼容性更改。修改表或存储字段时，请同步更新此文件中的 schema 和兼容性迁移，保留现有用户数据，并添加 Rust 迁移测试。

```bash
# 验证 Rust 服务、持久化和 schema 兼容性
pnpm test:rust
```

无需单独生成 seed 数据库。

---

## 文档更新

当进行影响用户体验的更改时，请同时更新相应文档：

| 更改内容 | 需要更新 |
|----------|----------|
| 功能或 UI | `README.md`、`docs/README.ko.md`、`docs/README.zh.md` |
| 贡献流程 | `CONTRIBUTING.md`、`docs/CONTRIBUTING.ko.md`、`docs/CONTRIBUTING.zh.md` |
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
