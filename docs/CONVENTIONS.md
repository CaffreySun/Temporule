# Temporule（拾律）项目规范

> 本文件是项目唯一规范来源。所有代码、提交、CI、子代理任务分发都以本文件为准。
> 变更本文件 = 变更项目契约，需走 task-spec 的 Challenge 流程。

## 1. 技术栈与架构规范

### 技术栈（已锁定）
- **语言**：Rust（core + CLI + Tauri 后端）、TypeScript（前端，无框架）
- **GUI**：Tauri 2（系统 WebView）；跨平台设计，本期仅 macOS 实机验证
- **CLI**：clap 4（derive），交互提示用 stdin 逐行（不引 dialoguer）
- **存储**：JSON 原子写（tmp + rename）+ jsonl 历史；不引入 SQLite/服务端
- **禁止**：Qt、Python、Electron、重量级前端框架（React/Vue 等）

### 依赖方向（硬约束）
```
temporule-cli ─┐
               ├─→ temporule-core
temporule-app ─┘
```
- `temporule-core` **不得**依赖任何 GUI/系统 UI/通知 crate；只依赖纯逻辑 crate（chrono、serde、uuid、thiserror、directories、notify 允许）
- `temporule-cli` 不得依赖 Tauri；`temporule-app`（src-tauri）不得包含调度/存储业务逻辑——只能调用 core
- 违反依赖方向 = 构建失败级缺陷

### 跨平台就绪规范
- 一切平台相关 API 必须经已抽象的 crate（`directories`、`notify`、`tauri-plugin-*`），禁止裸 `#[cfg]` 散落；确实需要的平台分支集中在 core 的 `platform` 模块并附注释

## 2. 代码规范

### Rust
- `rustfmt` 默认配置，CI 跑 `cargo fmt --all -- --check`
- clippy：`cargo clippy --all-targets -- -D warnings` 在 CI 零警告通过；禁用 lint 必须逐条注释理由
- 错误处理：core 用 `thiserror` 定义可穷举错误类型；二进制（cli / src-tauri）用 `anyhow` 收口
- 公共 API 必须有中文 rustdoc；`#![forbid(unsafe_code)]` 于 core
- 命名：模块/函数 snake_case；类型 PascalCase；常量 SCREAMING_SNAKE_CASE
- 时间一律 `chrono::DateTime<Local>` + `NaiveTime`，禁止裸时间戳整数

### TypeScript / 前端
- `tsc --strict` 零错误；`prettier --check` 通过（配置见 app/.prettierrc）
- 无框架、无运行时依赖的状态管理；DOM 操作原生；组件 = 一个返回 HTMLElement 的函数
- 命名：变量/函数 camelCase，类型 PascalCase，文件 kebab-case
- UI 文案一律中文（zh-CN）

### 测试代码
- 与产品代码同等标准：可读性优先于聪明；测试名描述行为（`每天窗口内间隔触发序列正确`对应英文描述式命名：`fires_at_window_anchor_steps_within_window`）
- fixture 用字面时间构造，禁止与实现同构的计算生成期望值（反恒真）

## 3. TDD 规范（强制）

- **红 → 绿循环**：先写失败测试，再写最小实现；禁止先实现后补测试
- **垂直切片**：一个测试 → 一个最小实现 → 下一个；禁止一次写完全部测试再实现（水平切片）
- **接缝**：只测公开接口。已锁定接缝：
  1. core：`next_fire` / `missed_occurrences` 纯函数
  2. core：`Store::load` / `save` / `append_history`（临时目录 fixture）
  3. CLI：进程级集成测试（assert_cmd），断言 stdout/退出码/文件副作用
  4. GUI/Tauri：不写自动化测试，实机验证（build + 截图 + history 输出）
- **新增接缝需经用户确认**（tdd skill 硬性要求）
- 禁止：测私有函数、mock 内部协作者、恒真断言、快照即实现

## 4. 文件管理规范

### 目录结构（固定，新增顶层目录需说明理由）
```
Temporule/
├── .task_spec/          # task-spec 规格档案（入库）
├── .github/workflows/   # CI
├── app/                 # Tauri GUI（前端 + src-tauri）
├── assets/icons/        # 图标资产（源 PNG/SVG 入此，构建期产物不入库）
├── crates/              # Rust workspace 成员
│   ├── temporule-core/
│   └── temporule-cli/
├── docs/                # 项目文档（规范、设计说明）
├── scripts/             # 构建/安装/冒烟脚本
├── Cargo.toml           # workspace 根
├── Makefile             # 常用命令入口
└── README.md
```

### 命名
- Rust 文件：snake_case.rs；TS/前端文件：kebab-case.ts；文档：kebab-case.md
- 分支：`feat/xxx`、`fix/xxx`、`chore/xxx`；一个切片一分支，合并后删除

### 禁止入库
`target/`、`node_modules/`、`dist/`、`*.dmg`、`*.app/`、`state.json`、`history.jsonl`、`.DS_Store`

### 数据文件
运行时数据只写 `~/Library/Application Support/com.temporule/`（由 `directories` 决定）；**禁止**在项目目录内写运行时状态。

## 5. Git 与提交规范

- 主分支 `main`；**单人维护期**：代码改动允许 main 直接提交，条件是——切片原子提交（一个逻辑切片一个 commit）、提交前 `make verify` 绿、Conventional Commits；**协作期**恢复分支 + PR/MR 流程。并行子代理共享同一工作树，无法一人一分支，是单人期例外的直接原因
- **Conventional Commits**：`feat(cli): 新增 snooze 子命令`；`scope` ∈ {core, cli, gui, app, build, docs, spec}
- 提交信息中文或英文均可，描述**行为**而非改动列表
- 单次提交 = 一个逻辑切片；禁止无关杂烩提交

## 6. CI/CD 规范

### CI（`.github/workflows/ci.yml`，push 与 PR 触发）
1. `cargo fmt --all -- --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test --workspace`（含 CLI 集成测试）
4. macOS runner 上 `npm ci && npm run tauri build`（GUI 可构建）

### 发布
- 版本号 semver，三端同版本（workspace 统一 `version`）
- 发布物：`.dmg`、`temporule` CLI 二进制（zip）；CHANGELOG.md（Keep a Changelog 格式）随版本更新
- 发布流程：tag `vX.Y.Z` → release workflow 构建产物
- 本仓库尚未关联远端；CI 文件按 GitHub Actions 编写，托管变更时仅迁移 workflow 格式，门槛与步骤不变

## 7. 文档规范

- README.md：项目简介、安装（GUI + CLI）、快速上手、CLI 全命令示例
- rustdoc：core 公共 API 全覆盖（中文）
- 设计/决策记录：大于会话级的设计决策写入 `docs/` 并在 README 索引
- 用户可见文案（CLI 输出、UI）中文；日志/内部注释可中英混合

## 8. 子代理驱动开发规范

- 父进程持有：总规格（`.task_spec/`）、集成所有权、最终验收
- 子代理切片必须自带：切片目标、涉及文件、接缝契约、验收标准、**本切片不做什么**
- 子代理执行期间**不跑**全量 fmt/clippy/构建——只做切片内验证；父进程在集成时统一跑门禁
- 子代理产出 = 完整可编译切片 + 其接缝测试；不允许交付占位/stub
- 每个切片完成后：父进程跑 `make verify`（fmt + clippy + test）再继续下一个切片

## 9. Makefile 入口（唯一命令入口）

| 目标 | 行为 |
|---|---|
| `make test` | `cargo test --workspace` |
| `make lint` | `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` |
| `make verify` | `make lint && make test`（任何切片完成后的统一门禁） |
| `make build` | GUI release 构建（.app + dmg） |
| `make dev` | GUI 开发模式（tauri dev） |
| `make install-cli` | CLI 安装脚本 |
| `make smoke` | CLI 冒烟脚本 |
