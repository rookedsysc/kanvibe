<div align="center">

# KanVibe

**AI 代理任务管理看板**

用于实时管理 AI 编程代理（Claude Code 等）任务的基于 Web 的终端看板。
在浏览器中直接监控 tmux/zellij 会话，同时通过拖放看板追踪任务进度。
通过 [Claude Code Hooks](#claude-code-hooks---自动状态追踪) 自动追踪任务状态，无需手动更新。

[![Buy Me A Coffee](https://img.shields.io/badge/Buy%20Me%20A%20Coffee-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=black)](https://buymeacoffee.com/rookedsysc)

> 请我喝杯咖啡也不错，但说实话，贡献代码会让我更开心。:)

[EN](../README.md) | [KO](./README.ko.md)

</div>

<div align="center">

[![Demo Video](https://img.youtube.com/vi/PBST0RIqlAA/maxresdefault.jpg)](https://www.youtube.com/watch?v=PBST0RIqlAA)

*点击上方图片在 YouTube 上观看演示视频*

<table>
  <tr>
    <td><img src="./images/kanvibe1.png" alt="看板" width="100%"></td>
    <td><img src="./images/kanvibe2.png" alt="任务详情 & 终端" width="100%"></td>
  </tr>
  <tr>
    <td align="center"><em>看板</em></td>
    <td align="center"><em>任务详情 & 终端</em></td>
  </tr>
</table>

</div>

---

## 快速开始

### 1. 配置环境变量

```bash
cp .env.example .env
```

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `PORT` | Web 服务器端口 | `4885` |
| `DB_PORT` | PostgreSQL 端口 | `4886` |
| `KANVIBE_USER` | 登录用户名 | `admin` |
| `KANVIBE_PASSWORD` | 登录密码 | `changeme`（请修改！） |

### 2. 运行

```bash
bash start.sh
```

这一个命令会处理所有事情：依赖安装、PostgreSQL 启动、数据库迁移、构建和服务器启动。

在浏览器中打开 `http://localhost:4885`。

---

## 使用流程

### 1. 注册项目

在项目设置中使用 **fzf 风格的文件夹搜索** 查找并注册本地 git 仓库。KanVibe 会扫描目录并自动检测现有的 worktree 分支。

### 2. 创建任务

在看板中添加 TODO 任务。使用分支名创建任务时，KanVibe 会自动：
- 为该分支创建 **git worktree**
- 生成 **tmux window** 或 **zellij tab** 终端会话
- 将终端会话链接到任务

### 3. 在看板中工作

任务通过 5 个状态进行管理：**TODO** → **PROGRESS** → **PENDING** → **REVIEW** → **DONE**

通过拖放更改状态，或通过 [Claude Code Hooks](#claude-code-hooks---自动状态追踪) 自动转换。当任务移至 **DONE** 时，分支、worktree 和终端会话会**自动删除**。

### 4. 选择面板布局

每个任务的终端页面支持多种面板布局：

| 布局 | 说明 |
|------|------|
| **Single** | 全屏单面板 |
| **Horizontal 2** | 左右两分 |
| **Vertical 2** | 上下两分 |
| **Left + Right TB** | 左侧 + 右侧上下分割 |
| **Left TB + Right** | 左侧上下分割 + 右侧 |
| **Quad** | 四等分 |

每个面板可以配置自定义命令（如 `vim`、`htop`、`lazygit`、测试运行器等）。布局可以全局设置或按项目设置。

---

## 功能

### 看板
- 5 状态任务管理（TODO / PROGRESS / PENDING / REVIEW / DONE）
- 自定义任务排序
- 多项目筛选
- Done 列分页
- 基于 WebSocket 的实时更新

### Git Worktree 集成
- 创建基于分支的任务时自动创建 git worktree
- Worktree 扫描：自动将现有分支注册为 TODO 任务
- 任务移至 DONE 时自动清理分支 + worktree + 会话

### 终端会话（tmux / zellij）
- 同时支持 **tmux** 和 **zellij** 作为终端复用器
- 基于 xterm.js + WebSocket 的浏览器终端
- SSH 远程终端支持（读取 `~/.ssh/config`）
- Nerd Font 渲染支持

### Claude Code Hooks - 自动状态追踪
KanVibe 与 **Claude Code Hooks** 集成，自动追踪任务状态。任务通过 5 个状态进行管理：

| 状态 | 说明 |
|------|------|
| **TODO** | 任务创建时的初始状态 |
| **PROGRESS** | AI 正在处理任务 |
| **PENDING** | AI 向用户提出追问，等待用户回复 |
| **REVIEW** | AI 已完成工作，等待审查 |
| **DONE** | 任务完成 — 分支、worktree、终端会话会**自动删除** |

```
用户发送提示词            → 任务移至 PROGRESS
AI 追问 (AskUser)        → 任务自动转为 PENDING
用户回答                  → 任务恢复为 PROGRESS
AI 完成响应               → 任务移至 REVIEW
任务移至 DONE             → 分支 + worktree + 终端会话自动删除
```

通过 KanVibe 目录扫描注册项目时，Hook 会**自动安装**。Hook 脚本放置在项目的 `.claude/hooks/` 目录中。

#### Hook API 端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/api/hooks/start` | POST | 创建新任务 |
| `/api/hooks/status` | POST | 通过 `branchName` + `projectName` 更新任务状态 |

### 面板布局编辑器
- 6 种布局预设（Single、Horizontal 2、Vertical 2、Left+Right TB、Left TB+Right、Quad）
- 每个面板自定义命令配置
- 全局和按项目布局设置

### 国际化（i18n）
- 支持语言：韩语（ko）、英语（en）、中文（zh）
- 基于 next-intl

---

## 技术栈

| 类别 | 技术 |
|------|------|
| 前端/后端 | Next.js 16 (App Router) + React 19 + TypeScript |
| 数据库 | PostgreSQL 16 + TypeORM |
| 样式 | Tailwind CSS v4 |
| 终端 | xterm.js + WebSocket + node-pty |
| SSH | ssh2 (Node.js) |
| 拖放 | @hello-pangea/dnd |
| 国际化 | next-intl |
| 容器 | Docker Compose |

---

## 许可证

本项目使用 **AGPL-3.0** 许可证。您可以自由地用于开源目的进行使用、修改和扩展。此许可证不允许商业 SaaS 分发。详情请参阅 [LICENSE](../LICENSE)。

---

## 贡献

请参阅 [CONTRIBUTING.zh.md](./CONTRIBUTING.zh.md)。

---

## Inspired By

- [workmux](https://github.com/raine/workmux) — tmux workspace manager
- [vibe-kanban](https://github.com/BloopAI/vibe-kanban) — AI-powered Kanban board
