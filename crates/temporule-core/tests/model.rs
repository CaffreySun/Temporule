//! 数据模型接缝测试：只走 `temporule_core::model` 公开 API。

use chrono::{Local, NaiveTime, TimeZone};
use temporule_core::model::{
    Delivery, PopupAlert, Reminder, Repeat, Schedule, TimeWindow, ValidationError,
};

/// 构造满足全部模型不变量的基准提醒，测试按需覆写单一字段。
fn reminder() -> Reminder {
    Reminder {
        id: uuid::Uuid::new_v4(),
        title: "提肛".to_string(),
        note: None,
        repeat: Repeat::Daily,
        schedule: Schedule::At {
            times: vec![NaiveTime::from_hms_opt(9, 0, 0).unwrap()],
        },
        delivery: Delivery::Popup {
            alert: PopupAlert::AutoDismiss { seconds: 30 },
        },
        snooze_minutes: 10,
        snooze_enabled: false,
        catch_up: false,
        enabled: true,
        created_at: Local.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap(),
        updated_at: Local.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap(),
    }
}

fn hms(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

#[test]
fn validate_rejects_each_model_invariant_violation() {
    // 标题空白
    let mut r = reminder();
    r.title = "   ".to_string();
    assert_eq!(r.validate(), Err(ValidationError::BlankTitle));

    // 间隔分钟数为 0
    let mut r = reminder();
    r.schedule = Schedule::Every {
        interval_minutes: 0,
        window: TimeWindow {
            start: hms(9, 0),
            end: hms(18, 0),
        },
    };
    assert_eq!(r.validate(), Err(ValidationError::InvalidInterval));

    // 每周重复未指定星期
    let mut r = reminder();
    r.repeat = Repeat::Weekly(vec![]);
    assert_eq!(r.validate(), Err(ValidationError::EmptyWeekdays));

    // 固定时刻列表为空
    let mut r = reminder();
    r.schedule = Schedule::At { times: vec![] };
    assert_eq!(r.validate(), Err(ValidationError::EmptyTimes));

    // 零长窗口（end == start）
    let mut r = reminder();
    r.schedule = Schedule::Every {
        interval_minutes: 30,
        window: TimeWindow {
            start: hms(9, 0),
            end: hms(9, 0),
        },
    };
    assert_eq!(r.validate(), Err(ValidationError::InvalidWindow));

    // 窗口终点早于起点（跨零点不允许）
    let mut r = reminder();
    r.schedule = Schedule::Every {
        interval_minutes: 30,
        window: TimeWindow {
            start: hms(10, 0),
            end: hms(9, 0),
        },
    };
    assert_eq!(r.validate(), Err(ValidationError::InvalidWindow));
}

#[test]
fn validate_accepts_reminder_satisfying_invariants() {
    assert_eq!(reminder().validate(), Ok(()));

    let mut r = reminder();
    r.repeat = Repeat::Weekly(vec![chrono::Weekday::Mon, chrono::Weekday::Sun]);
    r.schedule = Schedule::Every {
        interval_minutes: 120,
        window: TimeWindow {
            start: hms(9, 0),
            end: hms(18, 0),
        },
    };
    assert_eq!(r.validate(), Ok(()));
}

#[test]
fn describe_schedule_produces_chinese_summaries() {
    // 契约示例：每天 9–18 点每 2 小时
    let mut r = reminder();
    r.schedule = Schedule::Every {
        interval_minutes: 120,
        window: TimeWindow {
            start: hms(9, 0),
            end: hms(18, 0),
        },
    };
    assert_eq!(r.describe_schedule(), "每天 09:00–18:00 每 2 小时");

    // 契约示例：每周一 09:00
    let mut r = reminder();
    r.repeat = Repeat::Weekly(vec![chrono::Weekday::Mon]);
    r.schedule = Schedule::At {
        times: vec![hms(9, 0)],
    };
    assert_eq!(r.describe_schedule(), "每周一 09:00");

    // 多星期 + 多时刻：星期按周一…周日排序去重，时刻按时间排序，顿号分隔
    let mut r = reminder();
    r.repeat = Repeat::Weekly(vec![
        chrono::Weekday::Wed,
        chrono::Weekday::Mon,
        chrono::Weekday::Mon,
    ]);
    r.schedule = Schedule::At {
        times: vec![hms(14, 30), hms(9, 0)],
    };
    assert_eq!(r.describe_schedule(), "每周一、周三 09:00、14:30");

    // 间隔不足整小时时用「分钟」
    let mut r = reminder();
    r.schedule = Schedule::Every {
        interval_minutes: 90,
        window: TimeWindow {
            start: hms(9, 0),
            end: hms(18, 0),
        },
    };
    assert_eq!(r.describe_schedule(), "每天 09:00–18:00 每 90 分钟");
}

#[test]
fn serde_wire_format_uses_weekday_names_and_rfc3339() {
    let mut r = reminder();
    r.repeat = Repeat::Weekly(vec![chrono::Weekday::Mon, chrono::Weekday::Wed]);
    r.delivery = Delivery::Notification { sound: true };

    let json = serde_json::to_value(&r).unwrap();
    // 星期用小写短名字符串
    assert_eq!(json["repeat"]["weekly"], serde_json::json!(["mon", "wed"]));
    // 枚举变体用 snake_case 字符串
    assert_eq!(
        json["delivery"],
        serde_json::json!({"notification": {"sound": true}})
    );
    assert_eq!(
        json["schedule"]["at"]["times"],
        serde_json::json!(["09:00:00"])
    );
    // 时间用 RFC3339
    let ts = json["created_at"].as_str().unwrap();
    let parsed = chrono::DateTime::parse_from_rfc3339(ts).unwrap();
    assert_eq!(parsed.with_timezone(&Local), r.created_at);

    // 往返一致
    let back: Reminder = serde_json::from_value(json).unwrap();
    assert_eq!(back, r);
}

#[test]
fn snooze_enabled_defaults_to_false_when_field_missing() {
    // 旧数据（无 snooze_enabled 字段）反序列化 → 默认关闭
    let mut r = reminder();
    r.snooze_enabled = true;
    let mut json = serde_json::to_value(&r).unwrap();
    json.as_object_mut().unwrap().remove("snooze_enabled");
    let back: Reminder = serde_json::from_value(json).unwrap();
    assert!(!back.snooze_enabled);
    assert_eq!(back.snooze_minutes, 10);
}

#[test]
fn snooze_enabled_serde_roundtrip_preserves_field() {
    for enabled in [false, true] {
        let mut r = reminder();
        r.snooze_enabled = enabled;
        r.snooze_minutes = 5;
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["snooze_enabled"], enabled);
        let back: Reminder = serde_json::from_value(json).unwrap();
        assert_eq!(back.snooze_enabled, enabled);
        assert_eq!(back.snooze_minutes, 5);
        assert_eq!(back, r);
    }
}

#[test]
fn serde_wire_format_serializes_history_status_as_lowercase() {
    let entry = temporule_core::model::HistoryEntry {
        id: uuid::Uuid::new_v4(),
        reminder_id: uuid::Uuid::new_v4(),
        title: "提肛".to_string(),
        scheduled_at: Local.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap(),
        status: temporule_core::model::HistoryStatus::Snoozed,
        at: Local.with_ymd_and_hms(2026, 6, 1, 9, 0, 3).unwrap(),
    };
    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(json["status"], serde_json::json!("snoozed"));
    let back: temporule_core::model::HistoryEntry = serde_json::from_value(json).unwrap();
    assert_eq!(back, entry);
}
