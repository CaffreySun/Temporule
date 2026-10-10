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

**录入方式**：用户拍板**仅结构化表单**（无自然语言解析）。放弃「智能一句话」输入——减少解析歧义面，v1 收敛范围（被弃方案优势：一句话录入更快；以录入速度换取可靠性，解析覆盖不足时仍要回退表单）。

**弹窗**：用户拍板**置顶浮窗 + 可配置**（自动消失时长可调，或强制手动处理）。系统通知横幅仅作为「通知」触达方式保留（被弃方案优势：横幅零打断；以可见性与强制交互换取不漏看）。

**错过处理**：用户拍板**跳过并记入历史**（不补发），避免开机弹 8 条。附带每条提醒一个「错过补发」开关（默认关），零成本保留扩展面（被弃方案优势：补发不丢重要提醒；以打扰风险换取，per-reminder 开关已覆盖该场景）。

**已自行拍板的细节**：间隔锚定在窗口起点（9:00 起每 2h → 9/11/13/15/17）；触发时刻含窗口终点（≤ end）；存储用 JSON 原子写 + 目录监听，不用 SQLite/IPC daemon（被弃方案 SQLite 优势：天然并发写安全；以零迁移、可 diff、实现简单换取，并发丢失用 rev 守卫兜底，见存储节）；提醒仅在 App 运行时触发（菜单栏常驻进程即调度器，开机自启保证常驻）；**CLI→GUI 唤起机制**：`temporule open` = `open temporule://dashboard`（deep link，tauri-plugin-deep-link 注册 scheme），失败退化为 `open -a Temporule` 仅启动/聚焦 App。

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

struct SnoozeRecord {               // 一次性稍后，consume-once
    id: Uuid, reminder_id: Uuid, due_at: DateTime<Local>, created_at: DateTime<Local>,
}

// 模型不变量（Reminder::validate 强制；CLI 解析 / GUI 表单 / store load 三处都调用）：
// - interval_minutes ≥ 1（=0 时序列不前进，引擎死循环）
// - Weekly(Vec) 非空（空列表 = 永不触发且无报错）
// - At(times) 非空；window.end > window.start（拒绝跨零点与零长窗口）
// - title 非空白
```

语义示例：
- 每天 9–18 点每 2h 提肛 → `Daily, Every{120, 09:00–18:00}, Popup`
- 每天 9–18 点每 1h 喝水 → `Daily, Every{60, 09:00–18:00}, Notification`
- 每周一 9 点开会 → `Weekly[Mon], At{[09:00]}, Notification`

### 存储（core）

- 目录：`directories::ProjectDirs` → macOS `~/Library/Application Support/com.temporule/`（跨平台路径由 crate 保证）
- `reminders.json`：`{ version: 1, rev: u64, reminders: [...] }`。**写协议**：每次保存 = 重读当前文件 → 比对待保存快照基于的 `rev` → 不一致则拒绝（`SaveConflict` 错误上抛调用方：GUI 提示冲突需重载，CLI 天然逐命令重读不受此影响）→ 一致则 `rev+1` 序列化。**原子写**：每次写用唯一 tmp 名（pid+uuid 后缀），fsync 后 rename——禁止共享固定 tmp 名（并发写会撕裂文件）；fsync 在 rename 前，崩溃不留空文件
- `history.jsonl`：常规 append-only，`{ id, reminder_id, title, scheduled_at, status, at }`，status ∈ fired/done/dismissed/snoozed/missed；**截断规则**：条数 > 1000 时原子重写为保留最后 800 条（唯一 tmp + rename，同 reminders.json 机制）；**唯一截断写者 = GUI 调度器**（tick 时检查并截断），CLI 与 GUI 前端对 history 一律只读，写入只经 core 的 `append_history` / `truncate_history` 公开 API
- `snoozes.json`：`{ version: 1, snoozes: [SnoozeRecord] }`，**consume-once 语义**（见 GUI 节步骤 2/3）：准点触发与过期清扫两种结局都原子移除；CLI 与 GUI 的「稍后」按钮都经 core `add_snooze` 写入
- `state.json`：`{ last_tick }`（调度心跳时间，错过检测基准）；GUI 每次 tick 原子写，CLI 不碰
- 并发总纲：多写者场景靠「唯一 tmp + fsync + rename + rev 守卫」组合保证——单文件永不被撕裂；逻辑冲突（过期快照覆盖）由 rev 守卫拒绝并上抛，不静默吞掉

### 调度引擎（core，纯函数，可单测）

- `next_fire(reminder, after: DateTime<Local>) -> Option<DateTime<Local>>`：**严格晚于 `after`** 的最近触发时刻；逐日扫描（上限 8 天），对 `Every` 取窗口起点锚定序列 `start + k*interval ≤ end`；对 `At` 取时刻列表；按 `Repeat` 过滤星期。8 天无结果返回 None。输入不满足模型不变量（interval=0、空星期等）时 debug_assert + 返回 None——正常路径由 validate 拦截
- `occurrences(reminder, from_exclusive, to_inclusive) -> Vec<DateTime>`：半开区间 (from, to] 内全部应触发时刻（上限 500，超出截断并记 log）。**调度器不直接调 next_fire 判断触发**，而是每 tick 用本函数取 (last_tick, now] 全量处理（见 GUI 节）——这样 tick 延迟、睡眠间隙中落在窗口内的时刻不会既不 fired 也不 missed
- `missed_scan_from(reminder, last_tick, now) -> DateTime`：返回 `max(last_tick, created_at, updated_at)`——启动错过扫描的 per-reminder 下界（提醒创建/改规则之前的时刻不构成「错过」）
- 时区：全部 `Local`；夏令时不存在/歧义小时按 naïve 时刻直接解释（记为已知限制）

### CLI（`temporule` 二进制，与 GUI 完全对等）

- `temporule list [--json]`：标题、排程摘要、启用状态、下次触发时间
- `temporule add`：**flags 全量必填模式**（无 flags 时打印必填项用法并退出码 2——不做交互式问答，属未要求的范围）：
  `--title X [--note X] (--daily | --weekly mon,wed) (--every 2h --window 9:00-18:00 | --at 9:00,14:30) (--notification [--sound] | --popup [--force | --auto-dismiss 30]) [--snooze 10] [--catch-up]`
  - 裸 `--popup` = `AutoDismiss{30}`；`--force` 与 `--auto-dismiss` 互斥（clap `conflicts_with`）；不满足模型不变量的输入 → stderr 明确错误 + 退出码 2
- `temporule edit <id> [同上任意子集]`：部分更新；保存走 rev 守卫，冲突报 `SaveConflict`
- `temporule enable|disable <id>`、`temporule remove <id>`：`remove` 为破坏性操作——需 `-y/--yes`，或 TTY 下交互确认 `确认删除「标题」？[y/N]`；GUI 删除同理弹确认框
- `temporule snooze <id> <10m|1h>`：经 core `add_snooze` 写 `snoozes.json`（见存储节）
- `temporule history [--limit N]`：只读
- `temporule open`：`open temporule://dashboard`（deep link），失败退化 `open -a Temporule`
- 输出：人类可读表格默认；`--json` 给机器。错误 → stderr 非零退出。

### GUI（Tauri 2，菜单栏常驻 = 调度器本体）

- `tauri.conf.json`：`app.windows` 不预建主窗口；`macOSPrivateApi` 不需要。Tray icon 用 `Temporule_64.png`，`.with_menu_on_left_click(false)`：左键点击发事件 → 显示/聚焦仪表盘窗口。
- **无 Dock 图标（菜单栏常驻）**：Tauri 默认 activation policy 是 Regular（会出现在 Dock/Cmd-Tab）——启动时 `app.set_activation_policy(Accessory)`（Rust 侧，等价 LSUIElement）。仪表盘/主窗口仍可正常显示。
- 仪表盘窗口：frameless=false，compact ~440×560，`alwaysOnTop=true`、`skipTaskbar=true`、不可最小化、失焦自动隐藏（可配置开关默认开）、定位屏幕右下（距 dock 16px）。内容：标题「拾律」+ 运行状态圆点、提醒列表（标题、排程摘要、启用 switch、下次触发）、「+ 新增」按钮（切到表单视图）、「打开主窗口」按钮。
- 主窗口：~860×620，Tabs：提醒（完整列表 + 编辑/删除/新增，删除弹确认框）、历史、设置（开机自启开关、默认声音、数据目录展示、仪表盘失焦隐藏开关）。
- 新增/编辑表单（仪表盘与主窗口共用组件）：标题、备注、重复（每天/每周 + 星期勾选）、排程（每隔 N 分/时 + 时间窗 起/止；或 固定时刻列表）、触达（系统通知/弹窗 + 弹窗子配置：自动消失秒数 或 强制处理 + 声音开关）、稍后分钟数、错过补发开关。表单保存前本地校验模型不变量；收到 `SaveConflict` → 提示「数据已被外部修改」并强制重载列表，编辑器不静默覆盖。
- 调度循环（Rust 侧 async）：每分钟对齐边界 tick（周期 P=60s），处理四步（顺序固定）：
  1. **间隙处理**：对每个 enabled 提醒取 `occurrences(reminder, last_tick, now)`——按时延二分：**`t ≥ now − P` 的时刻为准点**，无条件进步骤 4 记 fired；**`t < now − P` 的时刻为间隙**：`catch_up=true` → 立即触发（补发）；`catch_up=false` → 逐条记 history(missed)。**睡眠间隙（App 运行中合盖）由此天然覆盖**：唤醒后下一 tick 的 (last_tick, now] 包含全部错过时刻，无需额外系统事件监听
  2. **snooze 消费**：取 `snoozes.json` 中 `due_at ∈ (last_tick, now]` 的记录——记录指向的 reminder 已删除 → 静默移除并记 log（同步骤 3）；否则同时延二分：准点 → 触发对应提醒（弹窗/通知按 delivery）；间隙 → 按 reminder.catch_up 补发或记 missed。两种结局都原子移除记录（consume-once）
  3. **过期 snooze 清扫**（启动时与唤醒后 tick 都执行）：`due_at ≤ last_tick` 的记录（App 未运行期间到点）→ 原子移除后按对应 reminder 的 catch_up 补发或记 missed；记录指向的 reminder 已删除 → 静默移除并记 log
  4. **触发分发**：写 history(fired) + 发通知（tauri-plugin-notification）或经弹窗注册表建弹窗
  - 每次 tick 末原子写 `state.json.last_tick = now`
- **弹窗注册表（防堆积，关键约束）**：同一 reminder 同时最多 1 个存活弹窗（新触发替换/聚焦旧的）；全局同时存活上限 5 个，超出 FIFO 排队，有弹窗关闭时递补；自动消失计时以最近一次替换重置。弹窗 ~360×140，右下，`alwaysOnTop`、skipTaskbar；AutoDismiss 到时关窗记 dismissed；RequireAction 不关
- **弹窗按钮 → Tauri commands**：`popup_action(popup_id, "done"|"snooze")`：done 记 done 并关窗；snooze 按 reminder.snooze_minutes 调 core `add_snooze` 记 snoozed 并关窗。前端 command 清单：list / add / update / remove / toggle / history / settings / popup_action
- 启动时：对每个 enabled 提醒取 `from = missed_scan_from(reminder, last_tick, now)`，跑与 tick 步骤 1 相同的时延二分逻辑（准点 fired / 间隙按 catch_up 补发或 missed）；并执行一次步骤 3 的过期 snooze 清扫
- 开机自启：设置页开关 → `tauri-plugin-autostart`。
- 托盘右键菜单：仪表盘 / 主窗口 / 退出拾律。
- 通知权限：首次触发前请求；设置页可重新请求。

### 图标与打包

- `.app` 图标：`Temporule_AppIcon.png`(1024) → `icon.icns`（sips/iconutil）+ Tauri 需要的 png 集（32/128/256/512）。
- 托盘图标：`Temporule_64.png`（后续可换 template 图）。
- 产物：`npm run tauri build` → `src-tauri/target/release/bundle/macos/Temporule.app` 与 dmg。
- **签名姿态**：本地构建无开发者证书 → `tauri.conf.json` 显式 `identity: -`（ad-hoc 签名）。系统通知对未签名 bundle 可能静默失败——Verify #5 实机验证通知路径，失败则升级为 ad-hoc + `NSUserNotificationsUsageDescription`（Info.plist）排查；分发环境（他人下载）需 Apple Developer 签名 + 公证，记为 v2 范围。
- **开机自启**：`tauri-plugin-autostart`（macOS 后端 = LaunchAgent），非 SMAppService——设置页文案用「开机自启」不提 SMAppService；LaunchAgent 需 .app 位于稳定路径（/Applications），安装说明注明「拖入 Applications 后再开自启」。
- CLI 安装：根目录 `scripts/install-cli.sh`（cargo build --release + symlink；目标目录按序探测：`brew --prefix` → `~/.local/bin`；两者都不可写时打印手动 PATH 指南后失败）。

## How（实施步骤）

0. **Harness 与规范（已完成）**：`docs/CONVENTIONS.md` 规范总纲；Cargo workspace 骨架（core/cli 可编译，fmt/clippy/test 门禁绿）；GitHub 开源仓库（MIT、main 分支、Conventional Commits、Actions CI）；图标归入 `assets/icons/`；命令入口统一 `Makefile`。仓库：https://github.com/CaffreySun/Temporule
1. **workspace + core**：`Cargo.toml` workspace；core 定义模型（含不变量 validate + SnoozeRecord）/引擎（next_fire 严格后驱、occurrences 半开区间、missed_scan_from）/存储（rev 守卫 + 唯一 tmp + fsync + snoozes.json + history 截断）；TDD 垂直切片逐条写测试。验收：`cargo test -p temporule-core` 全绿。
2. **CLI**：clap 子命令全量实现（flags 模式，裸 add 打印用法退出码 2；remove 需 -y 或 TTY 确认；--force/--auto-dismiss 互斥；open 走 deep link）。验收：smoke 脚本 + assert_cmd 集成测试走完 add/list/edit/enable/disable/snooze/history/remove/open，退出码正确。
3. **Tauri 骨架**：`app/` 手动建（package.json、vite、tsconfig、最小 index.html）；`npm create tauri-app` 非交互失败风险高，改为手写 + `npm i` + `cargo add`；tray（左键弹仪表盘）+ activation policy Accessory + 仪表盘窗口 + 主窗口路由先通。验收：`npm run tauri build` 成功产出 .app。
4. **GUI 前端**：仪表盘 + 主窗口三 Tab + 表单组件；调 Tauri commands（list/add/update/remove/toggle/history/settings/popup_action）。验收：build 通过 + 实机打开 .app 见托盘图标（无 Dock 图标）、点击弹仪表盘。
5. **调度集成**：Rust 侧 tick 四步循环（间隙处理/snooze 消费/过期 snooze 清扫/触发分发）、通知、弹窗注册表、catch-up、history 截断、文件监听 reload。验收：实机建「每 1 分钟」测试提醒，观察弹窗出现（同 reminder 二次触发替换不堆叠）、按钮动作、history 记录；睡眠间隙错过记入 missed。
6. **打包与安装**：图标全套、ad-hoc 签名配置、release 构建、install-cli.sh（brew prefix 探测 + 回退）、README（安装/使用/CLI 文档）。

## Verify（验收标准）

| # | 标准 | 证据 |
|---|---|---|
| 1 | `cargo test -p temporule-core` 退出码 0，覆盖：next_fire 跨日/周过滤/窗口锚定/≤end/At；occurrences 半开区间；missed_scan_from 钳制（created_at/updated_at）；模型不变量（interval=0、空星期、空 times、end==start 拒绝）；store 往返、rev 冲突拒绝、原子写（唯一 tmp）、history 截断（>1000 → 恰 800 条） | 测试输出 |
| 2 | CLI smoke + assert_cmd 集成测试：全链路命令退出码正确，`list --json` 字段完整，`edit/enable/disable/snooze/history/remove`（remove 无 -y 拒绝）、`--force`/`--auto-dismiss` 互斥、裸 add 退出码 2 | 测试与 smoke 输出 |
| 3 | `npm run tauri build` 成功，`Temporule.app` 存在，`open` 后菜单栏出现托盘图标且 Dock/Cmd-Tab 无图标（activation policy Accessory） | 构建日志 + `screencapture` 截图 |
| 4 | 点击托盘图标弹出仪表盘：显示提醒列表、运行状态、新增按钮、打开主窗口按钮均可用 | 截图 |
| 5 | 实机触发：每 1 分钟测试提醒在窗口内弹出浮窗；同 reminder 二次触发替换旧弹窗（不堆叠）；「完成」记 done、「稍后」按 snooze 延迟（CLI 可查到 snooze 记录）、AutoDismiss 到时关闭；系统通知路径同样走通 | 截图 + `temporule history` 输出 |
| 6 | 错过两条路径：a) 手动改 `state.json` last_tick 为 3 小时前重启 app → history 出现 missed（catch_up=false 不弹窗）；b) App 运行中合盖睡眠再唤醒 → 同样记入 missed/catch_up 补发 | history 输出 |
| 7 | 交叉写入：GUI 打开表单未保存时 CLI `add` 成功 → GUI 保存收到 SaveConflict 提示且 CLI 新增记录未被覆盖 | 实机操作 + store 内容 |

## TDD 与执行方式（用户补充要求，已确认）

- **TDD 强制**：红 → 绿循环，垂直切片（一个测试 → 最小实现 → 下一个），不允许先实现后补测试。
- **测试接缝**（用户授权自定，已锁定）：
  1. 调度引擎纯函数 `next_fire` / `occurrences` / `missed_scan_from` —— 用 chrono 字面时间断言触发/错过序列（跨日、周过滤、窗口锚定、≤end、At 多时刻）与钳制语义；模型不变量拒绝用例
  2. 存储层 `Store::load/save/append_history/truncate_history/add_snooze` —— 临时目录 fixture：JSON 往返、rev 冲突拒绝、唯一 tmp 原子写、history >1000 → 恰 800、损坏文件容错
  3. CLI 集成测试（assert_cmd 跑二进制）—— stdout 表格/`--json`、退出码、reminders.json/snoozes.json 副作用、互斥 flag 拒绝；锁住 CLI/GUI 对等契约
  4. GUI/Tauri 侧不写自动化测试，走实机验证（build + 截图 + history 输出）
- **反模式禁令**：不测内部实现、不写恒真测试、不水平切片（禁止一次写完全部测试再实现）。
- **Subagent 驱动**：实施阶段各切片派 task 子代理执行（核心引擎 / 存储 / CLI / Tauri 骨架 / 前端 / 调度集成 / 打包），父进程持有规格与集成所有权，子代理带完整切片规格与接缝契约；每个子代理在派发前被告知已通过的 Challenge 基线。

## Risks & 早期信号

- Tauri 2 tray 左键行为在 macOS 受系统约束（默认弹菜单）：`with_menu_on_left_click(false)` 若失效，降级为左键弹菜单（菜单首项=仪表盘）。早期信号：骨架阶段第 3 步实机验证。
- notify 目录监听误触发自己写入：防抖 200ms + reload 前比对内容 hash，相同则跳过。
- 通知权限被拒：设置页可重试；权限缺失时弹窗路径不受影响。
- 弹窗堆积：注册表已约束（同 reminder 单实例、全局上限 5、FIFO 排队）；若 WebView 创建延迟导致递补时序错乱，降级为关闭一个旧弹窗再开新弹窗。早期信号：步骤 5 实机每分钟触发观察。
- cargo/npm 网络拉包失败：构建前置 `npm i` 和 `cargo fetch` 单独跑，失败重试。

## 修订记录（Challenge 轮 2，已回写本文件）

第一轮 Challenge 产出 14 条 findings（1 条首轮即解决，13 条幸存），全部回写：

**Major（机制设计，已落规格）**
1. 运行时间隙（tick 延迟/睡眠合盖）错过不记录 → tick 改为全量处理 (last_tick, now] 间隙，睡眠天然覆盖（GUI 调度循环节）
2. snooze 记录无类型/无存储/无消费语义 → 新增 SnoozeRecord + snoozes.json consume-once + App 未运行到点规则（模型/存储/GUI 节）
3. 启动错过扫描对新建提醒产生幻影 missed → `missed_scan_from` = max(last_tick, created_at, updated_at)（引擎节）
4. 共享固定 tmp 名并发撕裂 + 缺 fsync → 唯一 tmp（pid+uuid）+ fsync-before-rename 全文件适用（存储节）
5. 过期快照覆盖丢更新 → reminders.json 增加 rev 守卫，SaveConflict 上抛不静默（存储/CLI/GUI 节）
6. 弹窗无界堆叠 → 弹窗注册表：同 reminder 单实例替换、全局上限 5、FIFO 排队（GUI 节）

**Minor（规格修正，已落规格）**
7. 模型不变量：interval ≥ 1、Weekly 非空、At 非空、end > start、title 非空白 —— validate 三处强制（模型节）
8. 裸 `--popup` = AutoDismiss{30}；`--force`/`--auto-dismiss` 互斥（CLI 节）
9. remove 破坏性 → CLI 需 `-y`/TTY 确认，GUI 确认框（CLI/GUI 节）
10. install-cli 硬编码 /opt/homebrew → brew --prefix 探测 + ~/.local/bin 回退 + PATH 指南（打包节）
11. 砍掉未要求的 CLI 交互式 add（无 flags 打印用法退出码 2）（CLI/How 节）
12. Explore 决策补记被弃方案优势；`temporule open` 机制定为 deep link `temporule://dashboard`（Explore 节）
13. 设置页「SMAppService」文案与插件实现不一致 → 统一为 tauri-plugin-autostart（LaunchAgent），文案只说「开机自启」（GUI/打包节）
14. 附加：无 Dock 图标策略（activation policy Accessory）、ad-hoc 签名姿态（打包节）；验收标准扩充第 7 条（交叉写入）

## 修订记录（Challenge 轮 3，已回写本文件）

轮 2 收敛检查发现 3 条幸存，已回写：
- **M1**：tick 全量间隙处理丢失准点触发路径（默认提醒会永远 missed）→ 步骤 1 改为时延二分：`t ≥ now − P`（P=tick 周期 60s）为准点无条件 fired，`t < now − P` 才是间隙按 catch_up 分流；启动扫描同规则
- **M2**：过期 snooze 无消费路径（会被无条件迟到补发）→ tick 步骤 2 fetch 改 `due_at ∈ (last_tick, now]` 同样时延二分；新增步骤 3「过期 snooze 清扫」覆盖 App 未运行期间到点（consume-once 覆盖补发与 missed 两种结局；reminder 已删则静默移除）；启动时执行清扫
- **m1**：Verify #2 要求 CLI 确定性产出 SaveConflict（单次命令进程内 rev 恒新鲜，不可测且与接缝 #3 重复）→ 删除该子句，rev 冲突覆盖保留在 core 单测（Verify #1）

## 本轮不做（finding 评审基线，勿报为缺陷）

- 自然语言录入（用户拍板仅表单）
- Windows/Linux 构建与实机验证（仅保证代码跨平台就绪）
- iCloud 同步、统计/打卡 streak、自定义铃声、多语言（zh-CN 之外）
- 提醒触发不依赖 App 常驻的独立 daemon（CLI 本身不触发提醒，仅管理数据）
- 跨零点时间窗（23:00–07:00 这类）
- 夏令时不存在/歧义小时的特殊处理
