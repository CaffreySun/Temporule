# Temporule（拾律）设计规范 v1

> 视觉设计唯一契约。设计切片、UI 改动以本文件为准；变更需说明理由。
> 代码/流程规范见 `docs/CONVENTIONS.md`。

## 基调

macOS 原生极简（控制中心 / NSPanel 质感）。深色优先、跟随系统。无渐变、无阴影堆砌、无圆角滥用。

## 间距（4pt 基数）

| Token | 值 | 用途 |
|---|---|---|
| `--space-sm` | 8px | 组件内间距 |
| `--space-md` | 12px | 卡片内边距 |
| `--space-card` | 8px | 卡片之间 |
| `--space-lg` | 16px | 区块之间 |

## 圆角

| 对象 | 值 |
|---|---|
| 窗口 | 12px |
| 卡片 | 10px |
| 控件（按钮/输入框/chip/switch） | 6px |

## 字体（系统字体 SF Pro）

| 角色 | 字号/字重 | 用途 |
|---|---|---|
| 标题 | 15px / 600 | 窗口标题、卡片标题 |
| 正文 | 13px / 400 | 内容、按钮 |
| 辅助 | 11px / 400 | 摘要、时间、占位 |
| 数字 | 13px + `font-variant-numeric: tabular-nums` | 倒计时、时刻（等宽防跳动） |

## 语义色板（CSS 变量，深色基线）

| Token | 值 | 用途 |
|---|---|---|
| `--surface` | `#1f1f22` | 窗口背景 |
| `--surface-raised` | `#2a2a2e` | 卡片、输入框背景 |
| `--border` | `rgba(255,255,255,0.08)` | 分隔线、控件边框 |
| `--text` | `#e8e8ea` | 正文 |
| `--text-dim` | `#8e8e93` | 辅助文字 |
| `--accent` | `#0a84ff` | 主操作、选中态、switch 开 |
| `--success` | `#30d158` | 运行中 |
| `--warning` | `#ffd60a` | 稍后/过期 |
| `--danger` | `#ff453a` | 删除、校验错误 |

## 组件

- **Switch**：44×24 胶囊，滑块 20，开 = `--accent`，150ms 滑动过渡（自绘，不用原生 checkbox 裸样式）
- **按钮**：高 28、圆角 6；主按钮 `--accent` 底白字；危险按钮 `--danger` 底；hover 提亮 8%、active 压暗 8%
- **输入框**：1px `--border` 底、圆角 6；focus 边框 `--accent`
- **窗口标题区**（无标题栏窗口）：自绘；标题 + 关闭红点按钮（hover 显 ×）；标题区 `-webkit-app-region: drag`，按钮 `no-drag`
- **卡片**：`--surface-raised` 底、圆角 10、内边距 12、间距 8；hover 边框提亮

## 窗口行为

- 仪表盘 / 弹窗：无标题栏（`decorations(false)`）+ 自绘标题区；alwaysOnTop；不抢系统焦点
- 主窗口：常规标题栏

## 动效

一律 100–200ms ease-out，只用于：hover 态、switch 滑动、弹层出现。禁止装饰性动画。

## 落地约束

- 所有颜色/间距/圆角必须走上述 token（CSS 变量），禁止散落魔法值
- 新组件先在本文件登记再实现
