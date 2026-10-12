//! 子命令执行：数据目录解析、提醒构建与输出格式化。
//!
//! 退出码约定：用法与校验错误 → 2（[`UsageError`]）；IO 与运行时错误 → 1。
//! 用户可见文案一律中文。

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use chrono::{Local, NaiveTime};
use directories::ProjectDirs;
use serde::Serialize;
use temporule_core::engine;
use temporule_core::model::{
    Delivery, HistoryEntry, HistoryStatus, PopupAlert, Reminder, Repeat, Schedule, SnoozeRecord,
    TimeWindow,
};
use temporule_core::store::{Store, StoreError};
use uuid::Uuid;

use crate::cli::{
    AddArgs, Command, CommonArgs, EditArgs, HistoryArgs, IdArgs, ListArgs, RemoveArgs, SnoozeArgs,
};

/// 用法与校验错误（退出码 2）。
#[derive(Debug)]
pub struct UsageError(String);

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

/// 构造用法/校验错误。
fn usage(message: impl Into<String>) -> anyhow::Error {
    UsageError(message.into()).into()
}

/// 按错误类别给出退出码：用法与校验 → 2，IO 与运行时 → 1。
pub fn exit_code(err: &anyhow::Error) -> i32 {
    if err.downcast_ref::<UsageError>().is_some() {
        2
    } else {
        1
    }
}

/// 执行子命令。
pub fn run(command: Command) -> Result<()> {
    match command {
        Command::List(args) => list(args),
        Command::Add(args) => add(args),
        Command::Edit(args) => edit(args),
        Command::Enable(args) => set_enabled(args, true),
        Command::Disable(args) => set_enabled(args, false),
        Command::Snooze(args) => snooze(args),
        Command::History(args) => history(args),
        Command::Remove(args) => remove(args),
        Command::Open => open_dashboard(),
    }
}

/// `open`：唤起 GUI 仪表盘。
///
/// 机制 = 文件落点 IPC（CLI 原子写 `invoke.json`，运行中的 App 消费并弹窗）；
/// deep link AppleEvent 在部分环境不送达，故不作为依赖。App 未运行时退化
/// `open -a Temporule` 启动（启动后消费 invoke.json 弹仪表盘）；
/// 两者都失败才报运行时错误（退出码 1）。`TEMPORULE_NO_LAUNCH=1` 时跳过启动
/// （测试隔离，避免拉起真实 App）。
fn open_dashboard() -> Result<()> {
    open_store()?.write_invoke("show_dashboard")?;

    if std::env::var_os("TEMPORULE_NO_LAUNCH").is_some() {
        return Ok(());
    }
    let launch_ok = std::process::Command::new("open")
        .args(["-a", "Temporule"])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if launch_ok {
        return Ok(());
    }
    Err(anyhow!(
        "无法打开 Temporule：open -a Temporule 失败（invoke 请求已写入，App 下次启动时生效）"
    ))
}

/// `remove`：破坏性操作——需 `-y/--yes`，或 TTY 下交互确认 `确认删除「标题」？[y/N]`。
///
/// 删除不级联 snooze 记录（调度器清扫兜底，见规格存储节）。
fn remove(args: RemoveArgs) -> Result<()> {
    use std::io::{IsTerminal, Write};

    let store = open_store()?;
    let data = store.load().map_err(map_store)?;
    let index = find_index(&data.reminders, &args.id)?;
    let title = data.reminders[index].title.clone();

    if !args.yes {
        if !std::io::stdin().is_terminal() {
            return Err(usage(format!(
                "删除是破坏性操作：非交互环境请加 -y/--yes 确认（删除「{title}」）"
            )));
        }
        eprint!("确认删除「{title}」？[y/N] ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .map_err(|source| anyhow!("读取确认输入失败：{source}"))?;
        let answer = line.trim().to_ascii_lowercase();
        if answer != "y" && answer != "yes" {
            return Err(usage("已取消删除"));
        }
    }

    let mut reminders = data.reminders;
    reminders.remove(index);
    store.save(data.rev, &reminders).map_err(map_store)?;
    Ok(())
}

/// `history`：最近 `--limit` 条历史，新→旧；`--json` 输出完整字段。
fn history(args: HistoryArgs) -> Result<()> {
    let store = open_store()?;
    let entries = store.read_history(args.limit).map_err(map_store)?;
    let entries: Vec<HistoryEntry> = entries.into_iter().rev().collect();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }
    if entries.is_empty() {
        println!("（暂无历史记录）");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = entries
        .iter()
        .map(|entry| {
            vec![
                entry.at.format("%Y-%m-%d %H:%M:%S").to_string(),
                status_cn(entry.status).to_string(),
                entry.title.clone(),
            ]
        })
        .collect();
    print_table(&["时间", "状态", "标题"], &rows);
    Ok(())
}

/// 处理结果的中文显示。
fn status_cn(status: HistoryStatus) -> &'static str {
    match status {
        HistoryStatus::Fired => "已触发",
        HistoryStatus::Done => "已完成",
        HistoryStatus::Dismissed => "已忽略",
        HistoryStatus::Snoozed => "已稍后",
        HistoryStatus::Missed => "错过",
    }
}

/// `snooze <id> <10m|1h>`：按提醒挂一条一次性稍后记录（consume-once，交调度器消费）。
fn snooze(args: SnoozeArgs) -> Result<()> {
    let store = open_store()?;
    let data = store.load().map_err(map_store)?;
    let index = find_index(&data.reminders, &args.id)?;
    let reminder_id = data.reminders[index].id;

    let minutes = parse_duration_minutes(&args.duration)?;
    let now = Local::now();
    let title = data.reminders[index].title.clone();
    let record = SnoozeRecord {
        id: Uuid::new_v4(),
        reminder_id,
        due_at: now + chrono::TimeDelta::minutes(i64::from(minutes)),
        created_at: now,
    };
    store.add_snooze(&record).map_err(map_store)?;
    // 与 GUI「稍后」按钮对等：记一条 snoozed 历史（scheduled_at = 动作时刻，无底层 occurrence）
    store
        .append_history(&HistoryEntry {
            id: Uuid::new_v4(),
            reminder_id,
            title,
            scheduled_at: now,
            status: HistoryStatus::Snoozed,
            at: now,
        })
        .map_err(map_store)?;
    Ok(())
}

/// `enable` / `disable`：翻转启用状态并推进 rev。
fn set_enabled(args: IdArgs, enabled: bool) -> Result<()> {
    let store = open_store()?;
    let data = store.load().map_err(map_store)?;
    let index = find_index(&data.reminders, &args.id)?;
    let mut reminders = data.reminders;
    reminders[index].enabled = enabled;
    reminders[index].updated_at = Local::now();
    store.save(data.rev, &reminders).map_err(map_store)?;
    Ok(())
}

/// 数据目录：`TEMPORULE_DATA_DIR` 优先（测试隔离），否则 `com.temporule` 项目数据目录。
fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("TEMPORULE_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    ProjectDirs::from_path(PathBuf::from("com.temporule"))
        .map(|dirs| dirs.data_dir().to_path_buf())
        .ok_or_else(|| anyhow!("无法确定数据目录（找不到用户主目录）"))
}

/// 打开存储句柄（目录不存在时由 Store 创建）。
fn open_store() -> Result<Store> {
    let dir = data_dir()?;
    Store::open(&dir).map_err(map_store)
}

/// 把存储错误映射到退出码约定：不满足模型不变量 → 用法/校验（2），其余 → 运行时（1）。
fn map_store(err: StoreError) -> anyhow::Error {
    match err {
        StoreError::InvalidReminder { reason, .. } => usage(reason.to_string()),
        other => other.into(),
    }
}

/// `list`：默认表格（ID 前 8 / 标题 / 排程摘要 / 启用 / 下次触发），`--json` 给完整字段。
fn list(args: ListArgs) -> Result<()> {
    let store = open_store()?;
    let data = store.load().map_err(map_store)?;
    let now = Local::now();

    if args.json {
        let items: Vec<ListJson> = data
            .reminders
            .iter()
            .map(|reminder| ListJson {
                reminder,
                schedule_summary: reminder.describe_schedule(),
                next_fire: next_fire_display(reminder, now),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    let rows: Vec<Vec<String>> = data
        .reminders
        .iter()
        .map(|reminder| {
            vec![
                reminder.id.to_string()[..8].to_string(),
                reminder.title.clone(),
                reminder.describe_schedule(),
                if reminder.enabled { "是" } else { "否" }.to_string(),
                match next_fire_display(reminder, now) {
                    Some(fire) => fire.format("%Y-%m-%d %H:%M").to_string(),
                    None => "—".to_string(),
                },
            ]
        })
        .collect();
    if rows.is_empty() {
        println!("（暂无提醒）");
        return Ok(());
    }
    print_table(&["ID", "标题", "排程", "启用", "下次触发"], &rows);
    Ok(())
}

/// `add`：flags 全量必填模式，保存到 `reminders.json`。
fn add(args: AddArgs) -> Result<()> {
    let common = args.common;
    let now = Local::now();
    let reminder = Reminder {
        id: Uuid::new_v4(),
        title: common
            .title
            .clone()
            .ok_or_else(|| usage("必须提供 --title"))?,
        note: common.note.clone(),
        repeat: build_repeat(&common)?,
        schedule: build_schedule(&common)?,
        delivery: build_delivery(&common)?,
        snooze_minutes: common.snooze.unwrap_or(10),
        snooze_enabled: common.snooze.is_some() && !common.no_snooze,
        catch_up: common.catch_up,
        enabled: true,
        created_at: now,
        updated_at: now,
    };
    reminder
        .validate()
        .map_err(|reason| usage(reason.to_string()))?;
    let store = open_store()?;
    let data = store.load().map_err(map_store)?;
    let mut reminders = data.reminders;
    reminders.push(reminder);
    store.save(data.rev, &reminders).map_err(map_store)?;
    Ok(())
}

/// `edit`：按任意子集部分更新，保存走 rev 守卫。
fn edit(args: EditArgs) -> Result<()> {
    let store = open_store()?;
    let data = store.load().map_err(map_store)?;
    let index = find_index(&data.reminders, &args.id)?;
    let mut reminder = data.reminders[index].clone();
    let common = args.common;

    let mut changed = false;
    if let Some(title) = &common.title {
        reminder.title = title.clone();
        changed = true;
    }
    if common.note.is_some() {
        reminder.note = common.note.clone();
        changed = true;
    }
    changed |= apply_repeat(&common, &mut reminder)?;
    changed |= apply_schedule(&common, &mut reminder)?;
    changed |= apply_delivery(&common, &mut reminder)?;
    if let Some(minutes) = common.snooze {
        reminder.snooze_minutes = minutes;
        reminder.snooze_enabled = true;
        changed = true;
    }
    if common.no_snooze {
        reminder.snooze_enabled = false;
        changed = true;
    }
    if common.catch_up {
        reminder.catch_up = true;
        changed = true;
    }
    if !changed {
        return Err(usage("未指定任何修改项"));
    }
    reminder.updated_at = Local::now();
    reminder
        .validate()
        .map_err(|reason| usage(reason.to_string()))?;
    let mut reminders = data.reminders;
    reminders[index] = reminder;
    store.save(data.rev, &reminders).map_err(map_store)?;
    Ok(())
}

/// 按完整 UUID 或唯一前缀定位提醒下标。
fn find_index(reminders: &[Reminder], raw: &str) -> Result<usize> {
    let raw = raw.trim();
    if let Ok(uuid) = Uuid::parse_str(raw) {
        if let Some(index) = reminders.iter().position(|reminder| reminder.id == uuid) {
            return Ok(index);
        }
    }
    let matched: Vec<usize> = reminders
        .iter()
        .enumerate()
        .filter(|(_, reminder)| reminder.id.to_string().starts_with(raw))
        .map(|(index, _)| index)
        .collect();
    match matched.as_slice() {
        [index] => Ok(*index),
        [] => Err(usage(format!("找不到提醒：{raw}"))),
        _ => Err(usage(format!("ID 前缀不唯一：{raw}"))),
    }
}

/// 解析重复规则（`add` 必填）。
fn build_repeat(common: &CommonArgs) -> Result<Repeat> {
    if common.daily {
        return Ok(Repeat::Daily);
    }
    if let Some(days) = &common.weekly {
        return Ok(Repeat::Weekly(parse_weekdays(days)?));
    }
    Err(usage("必须提供 --daily 或 --weekly"))
}

/// 应用重复规则变更（`edit` 部分更新），返回是否发生变更。
fn apply_repeat(common: &CommonArgs, reminder: &mut Reminder) -> Result<bool> {
    if common.daily {
        reminder.repeat = Repeat::Daily;
        return Ok(true);
    }
    if let Some(days) = &common.weekly {
        reminder.repeat = Repeat::Weekly(parse_weekdays(days)?);
        return Ok(true);
    }
    Ok(false)
}

/// 解析排程（`add` 必填）。
fn build_schedule(common: &CommonArgs) -> Result<Schedule> {
    if let Some(duration) = &common.every {
        let window = common
            .window
            .as_deref()
            .ok_or_else(|| usage("--every 需要与 --window 一起使用"))?;
        return Ok(Schedule::Every {
            interval_minutes: parse_duration_minutes(duration)?,
            window: parse_window(window)?,
        });
    }
    if let Some(times) = &common.at {
        return Ok(Schedule::At {
            times: parse_times(times)?,
        });
    }
    Err(usage("必须提供 --every --window 或 --at"))
}

/// 应用排程变更（`edit` 部分更新），返回是否发生变更。
///
/// `--every` 与 `--window` 可单独给出，缺省一侧沿用既有间隔排程；
/// 既有排程为固定时刻时单独给出任一侧都报用法错误。
fn apply_schedule(common: &CommonArgs, reminder: &mut Reminder) -> Result<bool> {
    match (&common.every, &common.window) {
        (Some(duration), Some(window)) => {
            reminder.schedule = Schedule::Every {
                interval_minutes: parse_duration_minutes(duration)?,
                window: parse_window(window)?,
            };
            Ok(true)
        }
        (Some(duration), None) => {
            let Schedule::Every { window, .. } = reminder.schedule else {
                return Err(usage(
                    "--every 需要与 --window 一起使用（当前排程为固定时刻）",
                ));
            };
            reminder.schedule = Schedule::Every {
                interval_minutes: parse_duration_minutes(duration)?,
                window,
            };
            Ok(true)
        }
        (None, Some(window)) => {
            let Schedule::Every {
                interval_minutes, ..
            } = reminder.schedule
            else {
                return Err(usage(
                    "--window 需要与 --every 一起使用（当前排程为固定时刻）",
                ));
            };
            reminder.schedule = Schedule::Every {
                interval_minutes,
                window: parse_window(window)?,
            };
            Ok(true)
        }
        (None, None) => {
            if let Some(times) = &common.at {
                reminder.schedule = Schedule::At {
                    times: parse_times(times)?,
                };
                return Ok(true);
            }
            Ok(false)
        }
    }
}

/// 弹窗行为：`--force` 必须手动处理，否则自动消失（裸 `--popup` = 30 秒）。
fn popup_alert(common: &CommonArgs) -> PopupAlert {
    if common.force {
        PopupAlert::RequireAction
    } else {
        PopupAlert::AutoDismiss {
            seconds: common.auto_dismiss.unwrap_or(30),
        }
    }
}

/// 解析触达方式（`add` 必填）。
fn build_delivery(common: &CommonArgs) -> Result<Delivery> {
    if common.notification {
        if common.force || common.auto_dismiss.is_some() {
            return Err(usage("--force / --auto-dismiss 仅适用于弹窗（--popup）"));
        }
        return Ok(Delivery::Notification {
            sound: common.sound,
        });
    }
    if common.popup {
        if common.sound {
            return Err(usage("--sound 仅适用于系统通知（--notification）"));
        }
        return Ok(Delivery::Popup {
            alert: popup_alert(common),
        });
    }
    Err(usage("必须提供 --notification 或 --popup"))
}

/// 应用触达变更（`edit` 部分更新），返回是否发生变更。
fn apply_delivery(common: &CommonArgs, reminder: &mut Reminder) -> Result<bool> {
    if common.notification {
        // 与 `build_delivery` 对称：弹窗专属 flag 不得随 --notification 静默丢弃
        if common.force || common.auto_dismiss.is_some() {
            return Err(usage("--force / --auto-dismiss 仅适用于弹窗（--popup）"));
        }
        reminder.delivery = Delivery::Notification {
            sound: common.sound,
        };
        return Ok(true);
    }
    if common.popup {
        if common.sound {
            return Err(usage("--sound 仅适用于系统通知（--notification）"));
        }
        reminder.delivery = Delivery::Popup {
            alert: popup_alert(common),
        };
        return Ok(true);
    }
    let mut changed = false;
    if common.sound {
        match &mut reminder.delivery {
            Delivery::Notification { sound } => {
                *sound = true;
                changed = true;
            }
            Delivery::Popup { .. } => {
                return Err(usage("--sound 仅适用于系统通知（--notification）"));
            }
        }
    }
    if common.force || common.auto_dismiss.is_some() {
        match &mut reminder.delivery {
            Delivery::Popup { alert } => {
                *alert = popup_alert(common);
                changed = true;
            }
            Delivery::Notification { .. } => {
                return Err(usage("--force / --auto-dismiss 仅适用于弹窗（--popup）"));
            }
        }
    }
    Ok(changed)
}

/// 解析间隔时长（`30m` / `2h` / `90m`），返回分钟数。
fn parse_duration_minutes(text: &str) -> Result<u32> {
    let text = text.trim();
    let (number, multiplier) = match text.strip_suffix(['m', 'M']) {
        Some(rest) => (rest, 1u32),
        None => match text.strip_suffix(['h', 'H']) {
            Some(rest) => (rest, 60u32),
            None => return Err(usage(format!("无法解析间隔时长：{text}（示例：30m / 2h）"))),
        },
    };
    let value: u32 = number
        .parse()
        .map_err(|_| usage(format!("无法解析间隔时长：{text}（示例：30m / 2h）")))?;
    value
        .checked_mul(multiplier)
        .ok_or_else(|| usage(format!("间隔时长溢出：{text}")))
}

/// 解析时间窗（`9:00-18:00`）。
fn parse_window(text: &str) -> Result<TimeWindow> {
    let (start, end) = text
        .split_once('-')
        .ok_or_else(|| usage(format!("无法解析时间窗：{text}（示例：9:00-18:00）")))?;
    Ok(TimeWindow {
        start: parse_time(start.trim())?,
        end: parse_time(end.trim())?,
    })
}

/// 解析固定时刻列表（`9:00,14:30`）。
fn parse_times(text: &str) -> Result<Vec<NaiveTime>> {
    text.split(',')
        .map(|part| parse_time(part.trim()))
        .collect()
}

/// 解析单个时刻（`9:00`）。
fn parse_time(text: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(text, "%H:%M")
        .map_err(|_| usage(format!("无法解析时刻：{text}（示例：9:00）")))
}

/// 解析星期列表（`mon,wed`，大小写不敏感）。
fn parse_weekdays(text: &str) -> Result<Vec<chrono::Weekday>> {
    use chrono::Weekday;
    text.split(',')
        .map(|part| {
            let name = part.trim().to_ascii_lowercase();
            match name.as_str() {
                "mon" | "monday" => Ok(Weekday::Mon),
                "tue" | "tuesday" => Ok(Weekday::Tue),
                "wed" | "wednesday" => Ok(Weekday::Wed),
                "thu" | "thursday" => Ok(Weekday::Thu),
                "fri" | "friday" => Ok(Weekday::Fri),
                "sat" | "saturday" => Ok(Weekday::Sat),
                "sun" | "sunday" => Ok(Weekday::Sun),
                _ => Err(usage(format!("无法识别的星期：{part}（示例：mon,wed）"))),
            }
        })
        .collect()
}

/// 下次触发：启用中的提醒取引擎结果，停用或无触发为 `None`。
fn next_fire_display(
    reminder: &Reminder,
    now: chrono::DateTime<Local>,
) -> Option<chrono::DateTime<Local>> {
    if !reminder.enabled {
        return None;
    }
    engine::next_fire(reminder, now)
}

/// `list --json` 的单条输出：完整提醒字段 + 排程摘要 + 下次触发。
#[derive(Debug, Serialize)]
struct ListJson<'a> {
    #[serde(flatten)]
    reminder: &'a Reminder,
    schedule_summary: String,
    next_fire: Option<chrono::DateTime<Local>>,
}

/// 简单按字符数对齐打印表格。
fn print_table(header: &[&str], rows: &[Vec<String>]) {
    let mut widths: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.chars().count());
            }
        }
    }
    let format_row = |cells: Vec<String>| {
        let mut line = String::new();
        for (i, cell) in cells.iter().enumerate() {
            if i > 0 {
                line.push_str("  ");
            }
            let width = widths.get(i).copied().unwrap_or(0);
            line.push_str(&format!("{:width$}", cell, width = width));
        }
        line.trim_end().to_string()
    };
    println!(
        "{}",
        format_row(header.iter().map(|s| s.to_string()).collect())
    );
    for row in rows {
        println!("{}", format_row(row.clone()));
    }
}
