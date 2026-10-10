# Temporule（拾律）v1 — Spec

## What

macOS 菜单栏提醒应用 Temporule（拾律）：状态栏常驻，弹出悬浮仪表盘管理提醒；
支持「间隔循环 + 时间窗」与「固定时刻」两类排程、系统通知与置顶弹窗两种触达方式；
提供与 GUI 功能对等的 CLI。技术栈 Tauri 2 + Rust（跨平台设计，本周期仅交付 macOS 并实机验证）。

## Explore — 方向是被选择的，不是默认的

**技术栈（四选一，用户已拍板 Tauri 2 + Rust）**

| 方向 | 它更好的地方 | 否决理由 |
|---|---|---|
| Tauri 2 + Rust ✅ | 跨平台（Windows/Linux 后续近零重写）；GUI/CLI/core 三端共用 workspace；产物小 | macOS 原生感略弱于 Swift（webview 渲染 UI，可接受） |
| Swift + SwiftUI 全原生 | macOS 通知/托盘/动画体验最佳 | 跨平台时 GUI+CLI 全部重写 |
| Rust core + Swift 壳 | 托盘/通知最稳 | 双语两套构建 + IPC 桥接，复杂度最高 |
| Go + Fyne/Wails | 编译快 | Fyne 托盘弱；Wails 同为 webview 但生态不如 Tauri |

**录入方式**：用户拍板**仅结构化表单**（无自然语言解析）。放弃「智能一句话」输入——减少解析歧义面，v1 收敛范围。

**弹窗**：用户拍板**置顶浮窗 + 可配置**（自动消失时长可调，或强制手动处理）。系统通知横幅仅作为「通知」触达方式保留。

**错过处理**：用户拍板**跳过并记入历史**（不补发），避免开机弹 8 条。附带每条提醒一个「错过补发」开关（默认关），零成本保留扩展面。

**已自行拍板的细节**：间隔锚定在窗口起点（9:00 起每 2h → 9/11/13/15/17）；触发时刻含窗口终点（≤ end）；存储用 JSON 原子写 + 目录监听（不用 SQLite/IPC daemon）；UI 中文（zh-CN）；CLI 与 GUI 共享 store 文件，GUI 通过 notify 监听目录热加载；提醒仅在 App 运行时触发（菜单栏常驻进程即调度器，开机自启保证常驻）。

## 架构

```
Temporule/
├── Cargo.toml                      # workspace
├── crates/
│   ├── temporule-core/             # 模型 + 调度引擎 + 存储（纯 Rust，无 GUI 依赖）
│   └── temporule-cli/              # CLI 二进制（依赖 core）
├── app/                            # Tauri 2 GUI
│   ├── package.json, vite 配置     # 前端构建
│   ├── src/                        # 前端 TS + HTML + CSS（无框架）
│   └── src-tauri/                  # Tauri Rust 侧（依赖 core）
└── .task_spec/temporule-v1.md
```

### 数据模型（core）

```rust
enum Repeat { Daily, Weekly(Vec<Weekday>) }        // 每天 | 每周指定几天
enum Schedule {
    Every { interval_minutes: u32, window: TimeWindow },  // 每 N 分钟，窗内锚定起点
    At    { times: Vec<NaiveTime> },                      // 固定时刻列表
}
struct TimeWindow { start: NaiveTime, end: NaiveTime }   // end > start；跨零点不允许
enum Delivery {
    Notification { sound: bool },
    Popup { alert: PopupAlert },
}
enum PopupAlert {
    AutoDismiss { seconds: u32 },   // 默认 30
    RequireAction,                  // 强制手动处理
}
struct Reminder {
    id: Uuid, title: String, note: Option<String>,
    repeat: Repeat, schedule: Schedule, delivery: Delivery,
    snooze_minutes: u32,            // 「稍后」默认分钟数，默认 10
    catch_up: bool,                 // 错过补发，默认 false
    enabled: bool,
    created_at / updated_at: DateTime<Local>,
}
```

语义示例：
- 每天 9–18 点每 2h 提肛 → `Daily, Every{120, 09:00–18:00}, Popup`
- 每天 9–18 点每 1h 喝水 → `Daily, Every{60, 09:00–18:00}, Notification`
- 每周一 9 点开会 → `Weekly[Mon], At{[09:00]}, Notification`

### 存储（core）

- 目录：`directories::ProjectDirs` → macOS `~/Library/Application Support/com.temporule/`（跨平台路径由 crate 保证）
- `reminders.json`：`{ version: 1, reminders: [...] }`；写 = tmp 文件 + `rename` 原子替换
- `history.jsonl`：append-only，`{ id, reminder_id, title, scheduled_at, status, at }`，status ∈ fired/done/dismissed/snoozed/missed；上限 1000 条，超出截断头部
- `state.json`：`{ last_tick }`（调度心跳时间，错过检测基准）
- 并发：CLI 与 GUI 各自原子写整文件，last-write-wins；GUI 用 `notify` 监听目录，外部变更后 200ms 防抖 reload。冲突概率低（人在一侧操作时不会同时改另一侧），可接受。

### 调度引擎（core，纯函数，可单测）

- `next_fire(reminder, after: DateTime<Local>) -> Option<DateTime<Local>>`：从 `after` 起逐日扫描（上限 8 天），对 `Every` 取窗口起点锚定的时刻序列 `start + k*interval ≤ end`；对 `At` 取时刻列表；按 `Repeat` 过滤星期。8 天无结果返回 None。
- `missed_occurrences(reminder, from, to) -> Vec<DateTime>`：期间所有应触发时刻（上限 500，超出截断并记 log）。
- 时区：全部 `Local`；夏令时不存在/歧义小时按 naïve 时刻直接解释（记为已知限制）。

### CLI（`temporule` 二进制，与 GUI 完全对等）

- `temporule list [--json]`：标题、排程摘要、启用状态、下次触发时间
- `temporule add`（无参数进入交互式逐项提问；或全 flags）：
  `--title X [--note X] (--daily | --weekly mon,wed) (--every 2h --window 9:00-18:00 | --at 9:00,14:30) (--notification [--sound] | --popup [--force | --auto-dismiss 30]) [--snooze 10] [--catch-up]`
- `temporule edit <id> [同上任意子集]`：部分更新
- `temporule enable|disable <id>`、`temporule remove <id>`
- `temporule snooze <id> <10m|1h>`：写一次性 snooze 记录，App 运行时被调度器消费
- `temporule history [--limit N]`
- `temporule open`：唤起 App 并聚焦仪表盘
- 输出：人类可读表格默认；`--json` 给机器。错误 → stderr 非零退出。

### GUI（Tauri 2，菜单栏常驻 = 调度器本体）

- `tauri.conf.json`：`app.windows` 不预建主窗口；`macOSPrivateApi` 不需要。Tray icon 用 `Temporule_64.png`，`.with_menu_on_left_click(false)`：左键点击发事件 → 显示/聚焦仪表盘窗口。
- 仪表盘窗口：frameless=false，compact ~440×560，`alwaysOnTop=true`、`skipTaskbar=true`、不可最小化、失焦自动隐藏（可配置开关默认开）、定位屏幕右下（距 dock 16px）。内容：标题「拾律」+ 运行状态圆点、提醒列表（标题、排程摘要、启用 switch、下次触发）、「+ 新增」按钮（切到表单视图）、「打开主窗口」按钮。
- 主窗口：~860×620，Tabs：提醒（完整列表 + 编辑/删除/新增）、历史、设置（开机自启 SMAppService、默认声音、数据目录展示、仪表盘失焦隐藏开关）。
- 新增/编辑表单（仪表盘与主窗口共用组件）：标题、备注、重复（每天/每周 + 星期勾选）、排程（每隔 N 分/时 + 时间窗 起/止；或 固定时刻列表）、触达（系统通知/弹窗 + 弹窗子配置：自动消失秒数 或 强制处理 + 声音开关）、稍后分钟数、错过补发开关。
- 调度循环（Rust 侧 async）：每分钟对齐边界 tick → 对每个 enabled 提醒 `next_fire(now)` ≤ now 即触发：写 history(fired)、发通知（tauri-plugin-notification）或建弹窗窗口（label `popup-{id}-{ts}`，~360×140，右下，`alwaysOnTop`、skipTaskbar；AutoDismiss 到时关窗记 dismissed；RequireAction 不关；按钮「完成」记 done、「稍后」按 snooze_minutes 建一次性 snooze 并记 snoozed）。每次 tick 更新 `state.json.last_tick`。
- 启动时：比对 `last_tick` → 对每个 enabled 提醒 `missed_occurrences(last_tick, now)`：`catch_up=true` 入队立即补发；否则逐条记 history(missed)。
- 开机自启：设置页开关 → `tauri-plugin-autostart`。
- 托盘右键菜单：仪表盘 / 主窗口 / 退出拾律。
- 通知权限：首次触发前请求；设置页可重新请求。

### 图标与打包

- `.app` 图标：`Temporule_AppIcon.png`(1024) → `icon.icns`（sips/iconutil）+ Tauri 需要的 png 集（32/128/256/512）。
- 托盘图标：`Temporule_64.png`（后续可换 template 图）。
- 产物：`npm run tauri build` → `src-tauri/target/release/bundle/macos/Temporule.app` 与 dmg。
- CLI 安装：根目录 `scripts/install-cli.sh`（cargo build --release + symlink 到 `/opt/homebrew/bin/temporule`，已存在则提示）。

## How（实施步骤）

1. **workspace + core**：`Cargo.toml` workspace；core 定义模型/引擎/存储；单测覆盖：next_fire（跨日、每周过滤、窗口锚定、≤end、At 多时刻）、missed_occurrences、store 读写往返 + 原子写、history 截断。验收：`cargo test -p temporule-core` 全绿。
2. **CLI**：clap 子命令全量实现；交互 add 用 stdin 逐行提问带默认值。验收：smoke 脚本走完 add/list/edit/enable/disable/snooze/history/remove/open，退出码正确。
3. **Tauri 骨架**：`app/` 手动建（package.json、vite、tsconfig、最小 index.html）；`npm create tauri-app` 非交互失败风险高，改为手写 + `npm i` + `cargo add`；tray + 仪表盘窗口 + 主窗口路由先通。验收：`npm run tauri build` 成功产出 .app。
4. **GUI 前端**：仪表盘 + 主窗口三 Tab + 表单组件；调 Tauri commands（list/add/update/remove/toggle/history/settings）。验收：build 通过 + 实机打开 .app 见托盘图标、点击弹仪表盘。
5. **调度集成**：Rust 侧 tick 循环、通知、弹窗窗口、snooze、catch-up、history、文件监听 reload。验收：实机建「每 1 分钟」测试提醒，观察弹窗出现、按钮动作、history 记录。
6. **打包与安装**：图标全套、release 构建、install-cli.sh、README（安装/使用/CLI 文档）。

## Verify（验收标准）

| # | 标准 | 证据 |
|---|---|---|
| 1 | `cargo test -p temporule-core` 退出码 0，覆盖 next_fire 跨日/周过滤/窗口锚定/≤end/At、missed、store 往返、history 截断 | 测试输出 |
| 2 | CLI smoke：`temporule add --title 提肛 --daily --every 2h --window 9:00-18:00 --popup` 等全链路命令退出码 0，`list --json` 字段完整，`edit/enable/disable/snooze/history/remove` 行为正确 | smoke 输出 |
| 3 | `npm run tauri build` 成功，`Temporule.app` 存在，`open` 后菜单栏出现托盘图标 | 构建日志 + `screencapture` 截图 |
| 4 | 点击托盘图标弹出仪表盘：显示提醒列表、运行状态、新增按钮、打开主窗口按钮均可用 | 截图 |
| 5 | 实机触发：每 1 分钟测试提醒在窗口内弹出浮窗；「完成」记 done、「稍后」按 snooze 延迟、AutoDismiss 到时关闭；系统通知路径同样走通 | 截图 + `temporule history` 输出 |
| 6 | 关机/睡眠错过：手动改 `state.json` last_tick 为 3 小时前，重启 app，history 出现 missed 记录（catch_up=false 不弹窗） | history 输出 |

## TDD 与执行方式（用户补充要求，已确认）

- **TDD 强制**：红 → 绿循环，垂直切片（一个测试 → 最小实现 → 下一个），不允许先实现后补测试。
- **测试接缝**（用户授权自定，已锁定）：
  1. 调度引擎纯函数 `next_fire` / `missed_occurrences` —— 用 chrono 字面时间断言触发/错过序列（跨日、周过滤、窗口锚定、≤end、At 多时刻、空 times、end==start 拒绝）
  2. 存储层 `Store::load/save/append_history` —— 临时目录 fixture：JSON 往返、原子写、history 1000 条截断、损坏文件容错
  3. CLI 集成测试（assert_cmd 跑二进制）—— stdout 表格/`--json`、退出码、reminders.json 副作用；锁住 CLI/GUI 对等契约
  4. GUI/Tauri 侧不写自动化测试，走实机验证（build + 截图 + history 输出）
- **反模式禁令**：不测内部实现、不写恒真测试、不水平切片（禁止一次写完全部测试再实现）。
- **Subagent 驱动**：实施阶段各切片派 task 子代理执行（核心引擎 / 存储 / CLI / Tauri 骨架 / 前端 / 调度集成 / 打包），父进程持有规格与集成所有权，子代理带完整切片规格与接缝契约；每个子代理在派发前被告知已通过的 Challenge 基线。

## Risks & 早期信号

- Tauri 2 tray 左键行为在 macOS 受系统约束（默认弹菜单）：`with_menu_on_left_click(false)` 若失效，降级为左键弹菜单（菜单首项=仪表盘）。早期信号：骨架阶段第 3 步实机验证。
- notify 目录监听误触发自己写入：防抖 200ms + reload 前比对内容 hash，相同则跳过。
- 通知权限被拒：设置页可重试；权限缺失时弹窗路径不受影响。
- WebView 弹窗多实例并发：label 带时间戳，无冲突。
- cargo/npm 网络拉包失败：构建前置 `npm i` 和 `cargo fetch` 单独跑，失败重试。

## 本轮不做（finding 评审基线，勿报为缺陷）

- 自然语言录入（用户拍板仅表单）
- Windows/Linux 构建与实机验证（仅保证代码跨平台就绪）
- iCloud 同步、统计/打卡 streak、自定义铃声、多语言（zh-CN 之外）
- 提醒触发不依赖 App 常驻的独立 daemon（CLI 本身不触发提醒，仅管理数据）
- 跨零点时间窗（23:00–07:00 这类）
- 夏令时不存在/歧义小时的特殊处理
