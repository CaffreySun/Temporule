# Temporule（拾律）

macOS 菜单栏常驻的周期提醒应用：状态栏图标 → 悬浮仪表盘 → 提醒管理。
支持「时间窗内每 N 分钟循环」与「固定时刻」两类排程，系统通知 / 置顶弹窗两种触达，
并提供与 GUI 完全对等的命令行工具 `temporule`。

> 开发中。当前进度与计划见 `.task_spec/temporule-v1.md`，项目规范见 `docs/CONVENTIONS.md`。

## 功能（规划）

- 每天 9:00–18:00 每 2 小时提醒「提肛」，每 1 小时提醒「喝水」
- 每周指定星期、固定时刻提醒（如每周一 9:00 开会）
- 两种触达：系统通知；置顶弹窗（可配置自动消失时长，或强制手动处理）
- 菜单栏悬浮仪表盘：运行状态一览、新增提醒、打开主窗口
- CLI 与 GUI 功能对等
- 电脑睡眠/关机错过的提醒记入历史（可逐条开启「错过补发」）

## 技术栈

Tauri 2 + Rust（core 调度引擎 / CLI / GUI 三端共用 workspace）、TypeScript 无框架前端。
跨平台设计，本期仅交付 macOS。

## 构建

```bash
make verify    # fmt + clippy + test（提交前必过）
make build     # 产出 Temporule.app 与 dmg
make install-cli   # 安装 temporule 命令
```

## CLI 速览（随开发同步更新）

```bash
temporule list                     # 列出提醒与下次触发时间
temporule add --title 提肛 --daily --every 2h --window 9:00-18:00 --popup
temporule edit <id> --title 喝水 --every 1h
temporule enable|disable <id>
temporule remove <id>
temporule history
```

## License

MIT
