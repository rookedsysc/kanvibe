# KanVibe 贡献指南

[EN](../CONTRIBUTING.md) | [KO](./CONTRIBUTING.ko.md)

感谢您对 KanVibe 的关注！本指南将帮助您开始贡献。

---

## 开发环境设置

### 前置要求

- Node.js 20+（参见 `.nvmrc`）
- pnpm
- Docker（用于 PostgreSQL）
- tmux 或 zellij（用于终端功能）

### 开始

```bash
# 克隆仓库
git clone https://github.com/rookedsysc/kanvibe.git
cd kanvibe

# 复制环境变量
cp .env.example .env

# 启动 PostgreSQL
docker compose up db -d

# 安装依赖
pnpm install

# 运行数据库迁移
pnpm migration:run

# 启动开发服务器
pnpm dev
```

在浏览器中打开 `http://localhost:4885`。

---

## Pull Request 指南

### 提交前检查

1. **所有检查必须通过：**
   ```bash
   pnpm build    # 构建成功
   pnpm check    # 类型检查通过
   pnpm test     # 测试通过
   ```

2. **附上工作截图或 GIF** 展示您的更改。没有功能运行视觉证据的 PR 将不会被合并。

3. **遵循现有代码风格。** 项目使用：
   - TypeScript 严格模式
   - Tailwind CSS v4 + 设计令牌（CSS 变量）
   - next-intl 处理所有面向用户的字符串
   - TypeORM 迁移进行模式更改

### PR 流程

1. Fork 仓库
2. 从 `main` 创建功能分支
3. 进行更改
4. 运行所有检查（`pnpm build && pnpm check && pnpm test`）
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

所有模式更改通过 TypeORM 迁移进行：

```bash
# 从实体更改生成迁移
pnpm migration:generate -- src/migrations/DescriptiveName

# 运行迁移
pnpm migration:run
```

永远不要使用 `synchronize: true`。详细的迁移工作流程请参见 `CLAUDE.md`。

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
