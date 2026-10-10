//! clap 命令定义：子命令、flags 与互斥/必填约束。
//!
//! 约束来源：`.task_spec/temporule-v1.md` 的「CLI」节。
//! `add` 为 flags 全量必填模式（缺必填项由 clap 报错退出码 2）；
//! `--force` 与 `--auto-dismiss` 互斥（`conflicts_with`）。

use clap::{ArgGroup, Args, Parser, Subcommand};

/// 命令行入口。
#[derive(Debug, Parser)]
#[command(name = "temporule", version, about = "Temporule（拾律）命令行工具")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// 子命令。
#[derive(Debug, Subcommand)]
pub enum Command {
    /// 列出全部提醒
    List(ListArgs),
    /// 新增提醒（flags 全量必填）
    Add(AddArgs),
    /// 编辑提醒（支持任意子集的部分更新）
    Edit(EditArgs),
    /// 启用提醒
    Enable(IdArgs),
    /// 停用提醒
    Disable(IdArgs),
    /// 稍后提醒（写入 snoozes.json）
    Snooze(SnoozeArgs),
    /// 查看历史记录（新→旧）
    History(HistoryArgs),
    /// 删除提醒（破坏性操作，需 -y 或交互确认）
    Remove(RemoveArgs),
    /// 唤起 GUI 仪表盘（deep link）
    Open,
}

/// `history` 参数。
#[derive(Debug, Args)]
pub struct HistoryArgs {
    /// 显示条数（默认 20）
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    /// 以 JSON 输出完整字段
    #[arg(long)]
    pub json: bool,
}

/// `snooze` 参数。
#[derive(Debug, Args)]
pub struct SnoozeArgs {
    /// 提醒 ID 或唯一前缀
    pub id: String,
    /// 稍后时长（如 10m / 1h）
    pub duration: String,
}

/// `remove` 参数。
#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// 提醒 ID 或唯一前缀
    pub id: String,
    /// 跳过确认直接删除
    #[arg(short = 'y', long = "yes")]
    pub yes: bool,
}

/// 仅需提醒 ID 的子命令参数。
#[derive(Debug, Args)]
pub struct IdArgs {
    /// 提醒 ID 或唯一前缀
    pub id: String,
}

/// `list` 参数。
#[derive(Debug, Args)]
pub struct ListArgs {
    /// 以 JSON 输出完整字段（id/title/schedule/enabled/next_fire 等）
    #[arg(long)]
    pub json: bool,
}

/// `add` 参数：flags 全量必填模式。
///
/// 必填组：标题、重复规则（--daily | --weekly）、排程（--every+--window | --at）、
/// 触达方式（--notification | --popup）。缺任一组 → clap 报错退出码 2。
#[derive(Debug, Args)]
#[command(group = ArgGroup::new("title_required").required(true).args(["title"]))]
#[command(group = ArgGroup::new("repeat").required(true).multiple(false).args(["daily", "weekly"]))]
#[command(group = ArgGroup::new("schedule").required(true).multiple(false).args(["every", "at"]))]
#[command(group = ArgGroup::new("delivery").required(true).multiple(false).args(["notification", "popup"]))]
pub struct AddArgs {
    #[command(flatten)]
    pub common: CommonArgs,
}

/// `edit` 参数：ID（完整 UUID 或唯一前缀）+ 任意子集 flags。
#[derive(Debug, Args)]
pub struct EditArgs {
    /// 提醒 ID 或唯一前缀
    pub id: String,
    #[command(flatten)]
    pub common: CommonArgs,
}

/// `add` 与 `edit` 共用的提醒字段 flags（`edit` 支持任意子集）。
#[derive(Debug, Args)]
pub struct CommonArgs {
    /// 标题
    #[arg(long)]
    pub title: Option<String>,
    /// 备注
    #[arg(long)]
    pub note: Option<String>,
    /// 每天重复
    #[arg(long, conflicts_with = "weekly")]
    pub daily: bool,
    /// 每周重复，逗号分隔星期（如 mon,wed）
    #[arg(long, value_name = "DAYS")]
    pub weekly: Option<String>,
    /// 间隔触发（如 30m / 2h / 90m），与 --window 成对使用
    #[arg(long, value_name = "DURATION", conflicts_with = "at")]
    pub every: Option<String>,
    /// 时间窗（如 9:00-18:00），与 --every 成对使用
    #[arg(long, value_name = "WINDOW", conflicts_with = "at")]
    pub window: Option<String>,
    /// 每日固定时刻列表（如 9:00,14:30）
    #[arg(long, value_name = "TIMES", conflicts_with_all = ["every", "window"])]
    pub at: Option<String>,
    /// 系统通知触达
    #[arg(long, conflicts_with = "popup")]
    pub notification: bool,
    /// 通知播放提示音
    #[arg(long)]
    pub sound: bool,
    /// 置顶弹窗触达（裸 --popup = 30 秒自动消失）
    #[arg(long)]
    pub popup: bool,
    /// 弹窗必须手动处理（与 --auto-dismiss 互斥）
    #[arg(long, conflicts_with = "auto_dismiss")]
    pub force: bool,
    /// 弹窗自动消失秒数（与 --force 互斥）
    #[arg(long, value_name = "SECONDS", conflicts_with = "force")]
    pub auto_dismiss: Option<u32>,
    /// 「稍后」默认分钟数（默认 10）
    #[arg(long)]
    pub snooze: Option<u32>,
    /// 错过补发
    #[arg(long)]
    pub catch_up: bool,
}
