# KanVibe 贡献指南

[EN](../CONTRIBUTING.md) | [KO](./CONTRIBUTING.ko.md)

感谢您对 KanVibe 的关注。

---

## 当前架构

KanVibe 现在是一个仅桌面的 Tauri 应用。

- 活跃运行时：Tauri v2 + Rust backend
- 桌面 UI 宿主：`src-tauri/desktop/index.html`
- Rust 入口：`src-tauri/src/main.rs`
- 本地持久化：基于 `rusqlite` 的 SQLite
- Next.js/Node Web 运行时已不再是活跃路径

---

## 开发环境设置

### 前置要求

- Node.js 22+
- pnpm
- Rust toolchain（`rustup`、`cargo`）
- 当前系统运行 Tauri 所需的 GTK/WebKit 依赖
- 如需验证终端/worktree 流程，安装 tmux 或 zellij

### 开始

```bash
# 克隆仓库
git clone https://github.com/rookedsysc/kanvibe.git
cd kanvibe

# 复制环境变量
cp .env.example .env

# 安装 JS 侧工具
pnpm install

# 启动桌面应用开发模式
pnpm dev
```

### 常用命令

```bash
pnpm dev            # 启动 Tauri 桌面应用开发模式
pnpm check          # 对 src-tauri 执行 cargo check
pnpm test           # 对 src-tauri 执行 cargo test
pnpm lint           # cargo fmt --check
pnpm build          # 构建桌面 release 二进制
pnpm desktop:qa     # 一次执行 check + test + build
```

---

## Tauri 开发流程

推荐的日常开发顺序如下：

1. 修改 `src-tauri/src/` 下的 Rust backend 代码
2. 如有需要，修改 `src-tauri/desktop/index.html` 桌面 UI 壳层
3. 运行 `pnpm check`
4. 运行 `pnpm test`
5. 用 `pnpm dev` 手动验证桌面应用行为

### 运行时说明

- Tauri 会加载 `src-tauri/tauri.conf.json` 中 `frontendDist` 指向的本地桌面资源
- 活跃 UI 通过 `window.__TAURI__.core.invoke(...)` 调用 Rust command
- SQLite schema 初始化逻辑目前位于 `src-tauri/src/backend/db.rs`

---

## 构建与发布

### 本地 Release 构建

```bash
pnpm build
```

当前 release 输出位置：

- 二进制：`src-tauri/target/release/kanvibe-desktop`

### 运行验证

构建完成后，请验证二进制可以实际启动。

```bash
./src-tauri/target/release/kanvibe-desktop
```

在无头 Linux 环境中，可以使用基于 Broadway 的 smoke test：

```bash
broadwayd :7 >/tmp/kanvibe-broadway.log 2>&1 &
GDK_BACKEND=broadway BROADWAY_DISPLAY=:7 timeout 8s ./src-tauri/target/release/kanvibe-desktop
```

如果进程一直存活到 timeout，则可视为一次成功的启动验证。

### 打包与分发

目前 `src-tauri/tauri.conf.json` 中的 `bundle.active` 为 `false`，因此默认 release 流程只会生成桌面二进制。

如果要生成安装包或平台原生 bundle：

1. 在 `src-tauri/tauri.conf.json` 中启用 bundling
2. 配置对应平台的 bundle target 与 signing
3. 再次执行 `pnpm build`

---

## Pull Request 指南

### 提交前检查

1. 所有检查必须通过：

```bash
pnpm check
pnpm test
pnpm build
```

2. 如果修改了可见的桌面行为，请附上截图或 GIF。
3. 所有更改都应符合桌面专用的 Tauri/Rust 架构。

### PR 流程

1. Fork 仓库
2. 从 `dev` 创建分支
3. 完成修改
4. 运行 `pnpm desktop:qa`
5. 用 `pnpm dev` 或 release 二进制启动验证功能
6. 使用 Conventional Commits 提交
7. Push 并创建 Pull Request

### 提交消息格式

```text
feat(scope): 添加新功能
fix(scope): 修复特定问题
docs: 更新文档
refactor(scope): 重构代码
```

---

## 数据库变更

活跃运行时已不再使用 TypeORM migration。

当你修改持久化逻辑时：

1. 更新 `src-tauri/src/backend/db.rs` 中的 schema 创建逻辑
2. 更新相关 Rust command 的读写逻辑
3. 仔细验证对已有本地 SQLite 数据的兼容性

由于应用使用本地 SQLite，除非有明确计划，否则不要进行破坏性 schema 变更。

---

## 文档更新

当修改影响用户或贡献者体验的行为时，请同步更新相关文档。

| 变更内容 | 需要更新 |
|----------|----------|
| 桌面行为或工作流 | `README.md`、`docs/README.ko.md`、`docs/README.zh.md` |
| 贡献或发布流程 | `CONTRIBUTING.md`、`docs/CONTRIBUTING.ko.md`、`docs/CONTRIBUTING.zh.md` |
| 环境变量 | `.env.example` + 所有 README |
| Tauri 构建/发布流程 | README + 所有 CONTRIBUTING 文档 |

请保持 EN、KO、ZH 三个版本同步。

---

## 许可证

向 KanVibe 提交贡献即表示您同意这些贡献将按照 [AGPL-3.0 许可证](../LICENSE) 进行许可。
