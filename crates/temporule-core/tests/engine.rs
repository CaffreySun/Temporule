//! 调度引擎接缝测试：只走 `temporule_core::engine` 公开纯函数。

use chrono::{DateTime, Local, NaiveTime, TimeZone};
use temporule_core::engine::{next_fire, occurrences};
use temporule_core::model::{Delivery, PopupAlert, Reminder, Repeat, Schedule};

/// 字面时间构造（契约测试接缝约定）。
fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
    Local.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
}

fn hms(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

/// 构造满足模型不变量的提醒；创建/修改时刻固定为 2026-06-01 08:00。
fn reminder(repeat: Repeat, schedule: Schedule) -> Reminder {
    Reminder {
        id: uuid::Uuid::new_v4(),
        title: "提肛".to_string(),
        note: None,
        repeat,
        schedule,
        delivery: Delivery::Popup {
            alert: PopupAlert::AutoDismiss { seconds: 30 },
        },
        snooze_minutes: 10,
        catch_up: false,
        enabled: true,
        created_at: dt(2026, 6, 1, 8, 0),
        updated_at: dt(2026, 6, 1, 8, 0),
    }
}

#[test]
fn fires_next_day_when_todays_time_already_passed() {
    // 2026-06-01 09:00 已过，下一次是次日 09:00（跨日）
    let r = reminder(
        Repeat::Daily,
        Schedule::At {
            times: vec![hms(9, 0)],
        },
    );
    assert_eq!(
        next_fire(&r, dt(2026, 6, 1, 10, 0)),
        Some(dt(2026, 6, 2, 9, 0))
    );
}

#[test]
fn fires_only_on_selected_weekdays_for_weekly_schedule() {
    // 每周一、周三 09:00（2026-06-01 是周一，06-03 是周三）
    let r = reminder(
        Repeat::Weekly(vec![chrono::Weekday::Mon, chrono::Weekday::Wed]),
        Schedule::At {
            times: vec![hms(9, 0)],
        },
    );
    // 周一 10:00 之后：跳过周二，命中周三
    assert_eq!(
        next_fire(&r, dt(2026, 6, 1, 10, 0)),
        Some(dt(2026, 6, 3, 9, 0))
    );
    // 周三 10:00 之后：越过周末，命中下周一
    assert_eq!(
        next_fire(&r, dt(2026, 6, 3, 10, 0)),
        Some(dt(2026, 6, 8, 9, 0))
    );
}

#[test]
fn fires_at_window_anchor_steps_within_window() {
    // 契约示例：每天 09:00–18:00 每 2 小时 → 9/11/13/15/17，次日从窗口起点重新锚定
    use temporule_core::model::TimeWindow;
    let r = reminder(
        Repeat::Daily,
        Schedule::Every {
            interval_minutes: 120,
            window: TimeWindow {
                start: hms(9, 0),
                end: hms(18, 0),
            },
        },
    );
    let mut cursor = dt(2026, 6, 1, 8, 0);
    for expected in [
        dt(2026, 6, 1, 9, 0),
        dt(2026, 6, 1, 11, 0),
        dt(2026, 6, 1, 13, 0),
        dt(2026, 6, 1, 15, 0),
        dt(2026, 6, 1, 17, 0),
        dt(2026, 6, 2, 9, 0),
    ] {
        let fired = next_fire(&r, cursor);
        assert_eq!(fired, Some(expected));
        cursor = fired.unwrap();
    }
}

#[test]
fn fires_at_window_end_when_end_lands_on_interval_grid() {
    // 窗口终点落在间隔网格上时必须触发（序列含 end）
    use temporule_core::model::TimeWindow;
    let r = reminder(
        Repeat::Daily,
        Schedule::Every {
            interval_minutes: 60,
            window: TimeWindow {
                start: hms(9, 0),
                end: hms(10, 0),
            },
        },
    );
    assert_eq!(
        next_fire(&r, dt(2026, 6, 1, 9, 30)),
        Some(dt(2026, 6, 1, 10, 0))
    );
    // 终点之后没有窗口内候选，跨日回到起点
    assert_eq!(
        next_fire(&r, dt(2026, 6, 1, 10, 0)),
        Some(dt(2026, 6, 2, 9, 0))
    );
}

#[test]
fn fires_at_each_configured_time_of_day() {
    // 固定时刻列表乱序给出，仍按时间序取最近触发
    let r = reminder(
        Repeat::Daily,
        Schedule::At {
            times: vec![hms(14, 30), hms(9, 0)],
        },
    );
    // 10:00 之后今天还剩 14:30
    assert_eq!(
        next_fire(&r, dt(2026, 6, 1, 10, 0)),
        Some(dt(2026, 6, 1, 14, 30))
    );
    // 15:00 之后跨日回到次日最早的 09:00
    assert_eq!(
        next_fire(&r, dt(2026, 6, 1, 15, 0)),
        Some(dt(2026, 6, 2, 9, 0))
    );
}

#[test]
fn returns_next_occurrence_when_after_equals_an_occurrence() {
    // 严格后驱：after 恰等于触发时刻时必须返回下一个，而不是它自己
    let r = reminder(
        Repeat::Daily,
        Schedule::At {
            times: vec![hms(9, 0), hms(14, 30)],
        },
    );
    assert_eq!(
        next_fire(&r, dt(2026, 6, 1, 9, 0)),
        Some(dt(2026, 6, 1, 14, 30))
    );

    use temporule_core::model::TimeWindow;
    let r = reminder(
        Repeat::Daily,
        Schedule::Every {
            interval_minutes: 120,
            window: TimeWindow {
                start: hms(9, 0),
                end: hms(18, 0),
            },
        },
    );
    assert_eq!(
        next_fire(&r, dt(2026, 6, 1, 9, 0)),
        Some(dt(2026, 6, 1, 11, 0))
    );
}

#[test]
fn occurrences_cover_half_open_interval() {
    let r = reminder(
        Repeat::Daily,
        Schedule::At {
            times: vec![hms(9, 0), hms(14, 30)],
        },
    );
    // 区间 (06-01 09:00, 06-02 09:00]：起点恰好的 09:00 不含，终点恰好的次日 09:00 含
    assert_eq!(
        occurrences(&r, dt(2026, 6, 1, 9, 0), dt(2026, 6, 2, 9, 0)),
        vec![dt(2026, 6, 1, 14, 30), dt(2026, 6, 2, 9, 0)]
    );
}

#[test]
fn occurrences_truncate_at_five_hundred() {
    use temporule_core::model::TimeWindow;
    // 每分钟一次，一天内 1439 个候选（区间内）；上限 500，超出截断
    let r = reminder(
        Repeat::Daily,
        Schedule::Every {
            interval_minutes: 1,
            window: TimeWindow {
                start: hms(0, 0),
                end: hms(23, 59),
            },
        },
    );
    let fires = occurrences(&r, dt(2026, 6, 1, 0, 0), dt(2026, 6, 1, 23, 59));
    assert_eq!(fires.len(), 500);
    assert_eq!(fires.first(), Some(&dt(2026, 6, 1, 0, 1)));
    assert_eq!(fires.last(), Some(&dt(2026, 6, 1, 8, 20)));
}

#[test]
fn missed_scan_from_takes_latest_of_tick_creation_and_update() {
    use temporule_core::engine::missed_scan_from;
    let mut r = reminder(
        Repeat::Daily,
        Schedule::At {
            times: vec![hms(9, 0)],
        },
    );

    // last_tick 最晚：钳制到 last_tick
    r.created_at = dt(2026, 6, 1, 8, 0);
    r.updated_at = dt(2026, 6, 1, 8, 30);
    assert_eq!(
        missed_scan_from(&r, dt(2026, 6, 1, 9, 0), dt(2026, 6, 1, 10, 0)),
        dt(2026, 6, 1, 9, 0)
    );

    // updated_at 最晚：钳制到 updated_at（改规则之前不构成「错过」）
    r.created_at = dt(2026, 6, 1, 8, 0);
    r.updated_at = dt(2026, 6, 1, 9, 30);
    assert_eq!(
        missed_scan_from(&r, dt(2026, 6, 1, 9, 0), dt(2026, 6, 1, 10, 0)),
        dt(2026, 6, 1, 9, 30)
    );

    // created_at 最晚（新建提醒不产生幻影 missed）
    r.created_at = dt(2026, 6, 1, 9, 45);
    r.updated_at = dt(2026, 6, 1, 8, 0);
    assert_eq!(
        missed_scan_from(&r, dt(2026, 6, 1, 9, 0), dt(2026, 6, 1, 10, 0)),
        dt(2026, 6, 1, 9, 45)
    );
}
