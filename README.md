# Temporule（拾律）

macOS 菜单栏常驻的周期提醒应用：状态栏图标 → 悬浮仪表盘 → 提醒管理。
支持「时间窗内每 N 分钟循环」与「固定时刻」两类排程，系统通知 / 置顶弹窗两种触达，
并提供与 GUI 完全对等的命令行工具 `temporule`。

## 功能

- 每天 9:00–18:00 每 2 小时提醒「活动提醒」，每 1 小时提醒「喝水」
- 每周指定星期、固定时刻提醒（如每周一 9:00 开会）
- 两种触达：系统通知（可带声音）；置顶弹窗（可配置自动消失时长，或强制手动处理）
- 菜单栏悬浮仪表盘：运行状态一览、新增/编辑/启停提醒、打开主窗口
- 主窗口：提醒管理 / 历史记录 / 设置（开机自启、默认声音、失焦隐藏）
- CLI 与 GUI 功能对等，共享同一数据目录
- 电脑睡眠/关机错过的提醒记入历史（可逐条开启「错过补发」）
- 「稍后」延后提醒（弹窗按钮或 CLI），一次性消费

## 安装

### GUI（macOS）

```bash
make build        # 产出 app/src-tauri/target/release/bundle/macos/Temporule.app 与 dmg
```

将 `Temporule.app` 拖入 `/Applications`。菜单栏出现图标即运行；开机自启需在设置页开启
（依赖稳定路径，务必先放入 /Applications）。

### CLI

```bash
make install-cli  # symlink 到 brew prefix 或 ~/.local/bin
```

## 使用

### CLI 全命令

```bash
temporule list [--json]            # 列出提醒与下次触发时间
temporule add --title 活动提醒 --daily --every 2h --window 9:00-18:00 --popup --auto-dismiss 30
temporule add --title 喝水 --daily --every 1h --window 9:00-18:00 --notification --sound
temporule add --title 周会 --weekly mon,wed --at 9:00 --notification
temporule edit <id> --every 45m    # 部分更新（id 支持唯一前缀）
temporule enable|disable <id>
temporule remove <id> -y           # 破坏性操作需 -y（TTY 可交互确认）
temporule snooze <id> 10m          # 一次性延后
temporule history [--limit N] [--json]
temporule open                     # 唤起 GUI 仪表盘（deep link）
```

`add` 参数：`( --daily | --weekly mon,wed )` × `( --every 2h --window 9:00-18:00 | --at 9:00,14:30 )`
× `( --notification [--sound] | --popup [--force | --auto-dismiss 30] )`，`--snooze 10` `--catch-up` 可选。
退出码：`0` 成功 / `2` 用法与校验 / `1` 运行时错误。

### 排程语义

- 「每 N 分钟」锚定在窗口起点：9:00 起每 2 小时 → 9 / 11 / 13 / 15 / 17 点
- 触发时刻含窗口终点（≤ end）；窗口不跨零点
- 错过判定：触发时刻晚于当前 1 个 tick 周期（60s）视为错过，按提醒的「错过补发」开关分流

## 数据与隐私

所有数据本地点存储，无网络请求：`~/Library/Application Support/com.temporule/`
（`reminders.json` / `snoozes.json` / `history.jsonl` / `state.json` / `settings.json`）。
可用 `TEMPORULE_DATA_DIR` 环境变量覆盖数据目录。

## 开发

```bash
make verify    # fmt + clippy -D warnings + test（提交前必过）
make dev       # GUI 开发模式
make smoke     # CLI 全链路冒烟（临时数据目录隔离）
```

技术栈：Tauri 2 + Rust workspace（core 调度引擎 / CLI / GUI），TypeScript 无框架前端。
规范见 `docs/CONVENTIONS.md`，需求规格与决策记录见 `.task_spec/temporule-v1.md`。

## License

MIT
