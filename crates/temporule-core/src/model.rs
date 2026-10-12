//! 提醒数据模型与模型不变量。
//!
//! 详见 `docs/CONVENTIONS.md` 与 `.task_spec/temporule-v1.md` 的「数据模型」节。
//! 模型不变量由 [`Reminder::validate`] 强制，CLI 解析 / GUI 表单 / 存储加载三处都调用。

use chrono::{DateTime, Local, NaiveTime, Weekday};
use serde::{Deserialize, Serialize};

/// 重复规则：每天，或每周指定的若干天。
///
/// 序列化为 snake_case 字符串/对象；星期用小写短名（`mon`…`sun`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Repeat {
    /// 每天重复。
    Daily,
    /// 每周在给定的星期几重复（至少一天）。
    Weekly(#[serde(with = "weekday_list")] Vec<Weekday>),
}

/// 时间窗（同一自然日内，跨零点不允许）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    /// 窗口起点（含）。
    pub start: NaiveTime,
    /// 窗口终点（含；必须晚于起点）。
    pub end: NaiveTime,
}

/// 排程：窗口内按固定间隔触发，或每日固定时刻触发。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Schedule {
    /// 每隔 `interval_minutes` 分钟触发一次，锚定窗口起点（`start + k*interval ≤ end`）。
    Every {
        /// 间隔分钟数，必须 ≥ 1。
        interval_minutes: u32,
        /// 触发时间窗，序列含窗口终点。
        window: TimeWindow,
    },
    /// 每日在给定的固定时刻触发（至少一个时刻）。
    At {
        /// 当天触发时刻列表。
        times: Vec<NaiveTime>,
    },
}

/// 弹窗行为配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PopupAlert {
    /// 显示若干秒后自动消失。
    AutoDismiss {
        /// 自动消失秒数（默认 30）。
        seconds: u32,
    },
    /// 必须手动处理，不自动关闭。
    RequireAction,
}

/// 触达方式：系统通知或置顶弹窗。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// 系统通知横幅。
    Notification {
        /// 是否播放提示音。
        sound: bool,
    },
    /// 置顶弹窗。
    Popup {
        /// 弹窗行为。
        alert: PopupAlert,
    },
}

/// 一条提醒的完整定义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reminder {
    /// 唯一标识。
    pub id: uuid::Uuid,
    /// 标题，非空白。
    pub title: String,
    /// 备注，可空。
    pub note: Option<String>,
    /// 重复规则。
    pub repeat: Repeat,
    /// 排程。
    pub schedule: Schedule,
    /// 触达方式。
    pub delivery: Delivery,
    /// 「稍后」默认分钟数（默认 10；仅 `snooze_enabled` 启用时生效）。
    pub snooze_minutes: u32,
    /// 「稍后提醒」是否启用（默认关闭；serde 反序列化缺省为 false，兼容旧数据）。
    #[serde(default)]
    pub snooze_enabled: bool,
    /// 错过是否补发（默认不补发）。
    pub catch_up: bool,
    /// 是否启用。
    pub enabled: bool,
    /// 创建时刻。
    pub created_at: DateTime<Local>,
    /// 最近修改时刻。
    pub updated_at: DateTime<Local>,
}

/// 模型不变量校验失败的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    /// 标题为空或仅空白字符。
    #[error("标题不能为空或仅空白字符")]
    BlankTitle,
    /// 间隔分钟数小于 1。
    #[error("间隔分钟数必须 ≥ 1")]
    InvalidInterval,
    /// 每周重复没有指定任何星期。
    #[error("每周重复至少需要指定一个星期")]
    EmptyWeekdays,
    /// 固定时刻列表为空。
    #[error("固定时刻列表不能为空")]
    EmptyTimes,
    /// 时间窗终点不晚于起点（零长窗口或跨零点）。
    #[error("时间窗终点必须晚于起点")]
    InvalidWindow,
}

impl Reminder {
    /// 校验模型不变量。
    ///
    /// 强制不变量：标题非空白、`interval_minutes ≥ 1`、每周重复非空、
    /// 固定时刻非空、`window.end > window.start`。
    /// 校验顺序：标题 → 重复规则 → 排程。
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.title.trim().is_empty() {
            return Err(ValidationError::BlankTitle);
        }
        match &self.repeat {
            Repeat::Daily => {}
            Repeat::Weekly(days) => {
                if days.is_empty() {
                    return Err(ValidationError::EmptyWeekdays);
                }
            }
        }
        match &self.schedule {
            Schedule::Every {
                interval_minutes,
                window,
            } => {
                if *interval_minutes < 1 {
                    return Err(ValidationError::InvalidInterval);
                }
                if window.end <= window.start {
                    return Err(ValidationError::InvalidWindow);
                }
            }
            Schedule::At { times } => {
                if times.is_empty() {
                    return Err(ValidationError::EmptyTimes);
                }
            }
        }
        Ok(())
    }

    /// 生成中文排程摘要，CLI 与 GUI 共用。
    ///
    /// 形如「每天 09:00–18:00 每 2 小时」「每周一 09:00」。
    /// 间隔能被 60 整除时以「小时」表述，否则以「分钟」表述；
    /// 星期按周一至周日排序去重，时刻按时间升序，均以顿号分隔。
    pub fn describe_schedule(&self) -> String {
        let repeat_part = match &self.repeat {
            Repeat::Daily => "每天".to_string(),
            Repeat::Weekly(days) => {
                let mut sorted = days.clone();
                sorted.sort_by_key(|d| d.num_days_from_monday());
                sorted.dedup();
                let names: Vec<&str> = sorted.iter().map(weekday_cn).collect();
                format!("每{}", names.join("、"))
            }
        };
        let schedule_part = match &self.schedule {
            Schedule::Every {
                interval_minutes,
                window,
            } => {
                let interval = if *interval_minutes % 60 == 0 {
                    format!("每 {} 小时", interval_minutes / 60)
                } else {
                    format!("每 {} 分钟", interval_minutes)
                };
                format!(
                    "{}–{} {}",
                    window.start.format("%H:%M"),
                    window.end.format("%H:%M"),
                    interval
                )
            }
            Schedule::At { times } => {
                let mut sorted = times.clone();
                sorted.sort();
                sorted
                    .iter()
                    .map(|t| t.format("%H:%M").to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            }
        };
        format!("{repeat_part} {schedule_part}")
    }
}

/// 星期几的中文显示名（周一…周日）。
fn weekday_cn(day: &Weekday) -> &'static str {
    match day {
        Weekday::Mon => "周一",
        Weekday::Tue => "周二",
        Weekday::Wed => "周三",
        Weekday::Thu => "周四",
        Weekday::Fri => "周五",
        Weekday::Sat => "周六",
        Weekday::Sun => "周日",
    }
}

/// 一次触发的处理结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryStatus {
    /// 已触发。
    Fired,
    /// 用户标记完成。
    Done,
    /// 用户忽略。
    Dismissed,
    /// 用户选择稍后。
    Snoozed,
    /// 错过未补发。
    Missed,
}

/// 历史记录条目（追加写入 `history.jsonl`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// 条目唯一标识。
    pub id: uuid::Uuid,
    /// 所属提醒标识。
    pub reminder_id: uuid::Uuid,
    /// 触发时的提醒标题快照。
    pub title: String,
    /// 计划触发时刻。
    pub scheduled_at: DateTime<Local>,
    /// 处理结果。
    pub status: HistoryStatus,
    /// 记录时刻。
    pub at: DateTime<Local>,
}

/// 一次性稍后记录（consume-once，见 `snoozes.json`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnoozeRecord {
    /// 记录唯一标识。
    pub id: uuid::Uuid,
    /// 所属提醒标识。
    pub reminder_id: uuid::Uuid,
    /// 稍后到点时刻。
    pub due_at: DateTime<Local>,
    /// 创建时刻。
    pub created_at: DateTime<Local>,
}

/// 星期序列的字符串序列化（小写短名 `mon`…`sun`）。
mod weekday_list {
    use chrono::Weekday;
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};

    /// 序列化为小写短名列表。
    pub fn serialize<S: Serializer>(days: &[Weekday], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(days.iter().map(short_name))
    }

    /// 反序列化：接受短名或全名，大小写不敏感。
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<Weekday>, D::Error> {
        let raw = Vec::<String>::deserialize(deserializer)?;
        raw.iter()
            .map(|name| {
                parse(name).ok_or_else(|| D::Error::custom(format!("无法识别的星期：{name}")))
            })
            .collect()
    }

    /// 星期的序列化短名。
    fn short_name(day: &Weekday) -> &'static str {
        match day {
            Weekday::Mon => "mon",
            Weekday::Tue => "tue",
            Weekday::Wed => "wed",
            Weekday::Thu => "thu",
            Weekday::Fri => "fri",
            Weekday::Sat => "sat",
            Weekday::Sun => "sun",
        }
    }

    /// 解析星期短名或全名。
    fn parse(name: &str) -> Option<Weekday> {
        match name.to_ascii_lowercase().as_str() {
            "mon" | "monday" => Some(Weekday::Mon),
            "tue" | "tuesday" => Some(Weekday::Tue),
            "wed" | "wednesday" => Some(Weekday::Wed),
            "thu" | "thursday" => Some(Weekday::Thu),
            "fri" | "friday" => Some(Weekday::Fri),
            "sat" | "saturday" => Some(Weekday::Sat),
            "sun" | "sunday" => Some(Weekday::Sun),
            _ => None,
        }
    }
}
