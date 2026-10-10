//! 调度引擎：纯函数计算触发时刻，无 IO、无全局状态。
//!
//! 公开接缝：[`next_fire`]、[`occurrences`]、[`missed_scan_from`]。
//! 输入不满足模型不变量时 debug 构建触发断言，并返回空结果（正常路径由
//! [`crate::model::Reminder::validate`] 拦截）。

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, NaiveTime, TimeZone};

use crate::model::{Reminder, Repeat, Schedule};

/// [`occurrences`] 单次返回的触发时刻上限，超出截断。
pub const MAX_OCCURRENCES: usize = 500;

/// [`next_fire`] 逐日扫描的天数上限（含 `after` 当天，最多向后 7 天）。
const SCAN_DAYS: u64 = 8;

/// 返回严格晚于 `after` 的最近触发时刻；8 天内无触发返回 `None`。
///
/// - [`Schedule::Every`]：每天锚定窗口起点，取 `start + k*interval ≤ end` 的序列；
/// - [`Schedule::At`]：取当天固定时刻；
/// - [`Repeat`] 过滤星期（跨零点时间窗不允许，见模型不变量）。
///
/// 时区夏令时不存在/歧义的小时按 naïve 时刻直接解释，无法映射的时刻跳过（已知限制）。
pub fn next_fire(reminder: &Reminder, after: DateTime<Local>) -> Option<DateTime<Local>> {
    let valid = reminder.validate().is_ok();
    debug_assert!(valid, "next_fire 要求提醒满足模型不变量：{reminder:?}");
    if !valid {
        return None;
    }
    let first_date = after.date_naive();
    for offset in 0..SCAN_DAYS {
        let Some(date) = first_date.checked_add_days(Days::new(offset)) else {
            break;
        };
        for time in day_candidates(reminder, date) {
            if let Some(fire) = to_local(date, time) {
                if fire > after {
                    return Some(fire);
                }
            }
        }
    }
    None
}

/// 某一天内的候选触发时刻（升序），已按 [`Repeat`] 过滤星期。
fn day_candidates(reminder: &Reminder, date: NaiveDate) -> Vec<NaiveTime> {
    let matched = match &reminder.repeat {
        Repeat::Daily => true,
        Repeat::Weekly(days) => days.contains(&date.weekday()),
    };
    if !matched {
        return Vec::new();
    }
    match &reminder.schedule {
        Schedule::At { times } => {
            let mut sorted = times.clone();
            sorted.sort();
            sorted
        }
        Schedule::Every {
            interval_minutes,
            window,
        } => {
            let mut times = Vec::new();
            let mut step = 0u32;
            loop {
                // 锚定窗口起点：start + k*interval，序列含窗口终点（≤ end）
                let delta =
                    chrono::TimeDelta::minutes(i64::from(step) * i64::from(*interval_minutes));
                let (time, day_overflow) = window.start.overflowing_add_signed(delta);
                if day_overflow != 0 || time > window.end {
                    break;
                }
                times.push(time);
                step += 1;
            }
            times
        }
    }
}

/// 返回半开区间 `(from_exclusive, to_inclusive]` 内的全部应触发时刻（升序）。
///
/// 超过 [`MAX_OCCURRENCES`] 的部分截断（调用方可据 `len() == MAX_OCCURRENCES` 判断截断并记录日志）。
/// 时区夏令时限制同 [`next_fire`]。
pub fn occurrences(
    reminder: &Reminder,
    from_exclusive: DateTime<Local>,
    to_inclusive: DateTime<Local>,
) -> Vec<DateTime<Local>> {
    let valid = reminder.validate().is_ok();
    debug_assert!(valid, "occurrences 要求提醒满足模型不变量：{reminder:?}");
    if !valid || to_inclusive <= from_exclusive {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut date = from_exclusive.date_naive();
    let last_date = to_inclusive.date_naive();
    loop {
        if date > last_date {
            break;
        }
        for time in day_candidates(reminder, date) {
            if let Some(fire) = to_local(date, time) {
                if fire > from_exclusive && fire <= to_inclusive {
                    if out.len() >= MAX_OCCURRENCES {
                        // 超出上限：截断，不再继续扫描
                        return out;
                    }
                    out.push(fire);
                }
            }
        }
        let Some(next) = date.checked_add_days(Days::new(1)) else {
            break;
        };
        date = next;
    }
    out
}

/// 启动错过扫描的 per-reminder 下界：`max(last_tick, created_at, updated_at)`。
///
/// 提醒创建/修改规则之前的时刻不构成「错过」，因此钳制到 `created_at` / `updated_at`；
/// `now` 是调用语境的扫描上界（GUI tick 的当前时刻），按契约不参与返回值计算。
pub fn missed_scan_from(
    reminder: &Reminder,
    last_tick: DateTime<Local>,
    now: DateTime<Local>,
) -> DateTime<Local> {
    let _ = now;
    reminder.created_at.max(reminder.updated_at).max(last_tick)
}

/// 把 naïve 日期 + 时刻映射为本地时间；夏令时不存在/歧义时取最早解释，无法映射返回 `None`。
fn to_local(date: NaiveDate, time: NaiveTime) -> Option<DateTime<Local>> {
    Local.from_local_datetime(&date.and_time(time)).earliest()
}
