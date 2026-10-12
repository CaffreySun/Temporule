//! 存储层接缝测试：tempfile 临时目录 fixture，只走 `temporule_core::store` 公开 API。

use chrono::{DateTime, Local, NaiveTime, TimeZone};
use temporule_core::model::{Delivery, PopupAlert, Reminder, Repeat, Schedule};
use temporule_core::settings::AppSettings;
use temporule_core::store::Store;

fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
    Local.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
}

fn hms(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

/// 构造满足模型不变量的提醒；创建/修改时刻固定为 2026-06-01 08:00。
fn reminder(title: &str) -> Reminder {
    Reminder {
        id: uuid::Uuid::new_v4(),
        title: title.to_string(),
        note: None,
        repeat: Repeat::Daily,
        schedule: Schedule::At {
            times: vec![hms(9, 0)],
        },
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
fn load_returns_empty_store_when_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let data = store.load().unwrap();
    assert_eq!(data.rev, 0);
    assert!(data.reminders.is_empty());
}

#[test]
fn save_and_load_round_trip_preserves_reminders_and_bumps_rev() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let a = reminder("提肛");
    let b = reminder("喝水");

    let rev = store.save(0, &[a.clone(), b.clone()]).unwrap();
    assert_eq!(rev, 1);

    let data = store.load().unwrap();
    assert_eq!(data.rev, 1);
    assert_eq!(data.reminders, vec![a, b]);
}

#[test]
fn save_rejects_stale_base_rev_with_conflict() {
    use temporule_core::store::StoreError;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let a = reminder("提肛");
    let b = reminder("喝水");

    store.save(0, std::slice::from_ref(&a)).unwrap();
    // 过期快照（基于 rev 0）覆盖会丢更新，必须拒绝
    let err = store.save(0, &[b]).unwrap_err();
    assert!(matches!(
        err,
        StoreError::SaveConflict {
            base_rev: 0,
            current_rev: 1
        }
    ));
    // 拒绝后文件内容不变
    let data = store.load().unwrap();
    assert_eq!(data.reminders, vec![a]);
}

#[test]
fn save_leaves_no_tmp_files_behind() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store.save(0, &[reminder("提肛")]).unwrap();

    // 原子写完成后：目录只剩 reminders.json，无任何 tmp 残留
    let names: Vec<std::ffi::OsString> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("reminders.json")]);
}

#[test]
fn load_backs_up_corrupt_file_and_returns_default() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let garbage = "{ 这不是合法 JSON";
    std::fs::write(dir.path().join("reminders.json"), garbage).unwrap();

    // 损坏绝不 panic：返回默认（rev 0 + 空列表）
    let data = store.load().unwrap();
    assert_eq!(data.rev, 0);
    assert!(data.reminders.is_empty());

    // 原内容被移到 *.corrupt-<ts> 备份，活动文件不再保留损坏数据
    let backups: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("reminders.json.corrupt-"))
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        std::fs::read_to_string(dir.path().join(&backups[0])).unwrap(),
        garbage
    );
    assert!(std::fs::read_to_string(dir.path().join("reminders.json")).is_err());
}

#[test]
fn save_rejects_reminder_that_violates_invariants() {
    use temporule_core::model::ValidationError;
    use temporule_core::store::StoreError;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();

    // 与 load 的强制校验对称：拒绝写入 load 会拒收的数据
    let mut bad = reminder("提肛");
    bad.schedule = Schedule::At { times: vec![] };
    let err = store.save(0, &[bad]).unwrap_err();
    match err {
        StoreError::InvalidReminder { reason, .. } => {
            assert_eq!(reason, ValidationError::EmptyTimes)
        }
        other => panic!("期望 InvalidReminder，实际 {other:?}"),
    }
    // 拒绝后不落盘
    assert!(std::fs::read_to_string(dir.path().join("reminders.json")).is_err());
}

#[test]
fn history_append_and_read_respect_limit() {
    use temporule_core::model::{HistoryEntry, HistoryStatus};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let reminder_id = uuid::Uuid::new_v4();

    let stamps = [
        dt(2026, 6, 1, 9, 0),
        dt(2026, 6, 1, 10, 0),
        dt(2026, 6, 1, 11, 0),
    ];
    let mut appended = Vec::new();
    for scheduled_at in stamps {
        let entry = HistoryEntry {
            id: uuid::Uuid::new_v4(),
            reminder_id,
            title: "提肛".to_string(),
            scheduled_at,
            status: HistoryStatus::Fired,
            at: scheduled_at,
        };
        store.append_history(&entry).unwrap();
        appended.push(entry);
    }

    // 全量读取：保持追加顺序
    assert_eq!(store.read_history(10).unwrap(), appended);
    // limit 取最近 N 条，仍按时间序
    assert_eq!(store.read_history(2).unwrap(), appended[1..].to_vec());

    // 无历史文件 → 空
    let empty_dir = tempfile::tempdir().unwrap();
    let empty_store = Store::open(empty_dir.path()).unwrap();
    assert!(empty_store.read_history(5).unwrap().is_empty());
}

#[test]
fn truncate_history_keeps_last_800_when_over_1000() {
    use temporule_core::model::{HistoryEntry, HistoryStatus};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();

    // 追加 1001 条（超过阈值 1000）
    let mut ids = Vec::new();
    for _ in 0..1001 {
        let entry = HistoryEntry {
            id: uuid::Uuid::new_v4(),
            reminder_id: uuid::Uuid::new_v4(),
            title: "提肛".to_string(),
            scheduled_at: dt(2026, 6, 1, 9, 0),
            status: HistoryStatus::Fired,
            at: dt(2026, 6, 1, 9, 1),
        };
        ids.push(entry.id);
        store.append_history(&entry).unwrap();
    }

    store.truncate_history().unwrap();
    let kept = store.read_history(2000).unwrap();
    // 恰保留最后 800 条
    assert_eq!(kept.len(), 800);
    assert_eq!(kept.first().unwrap().id, ids[201]);
    assert_eq!(kept.last().unwrap().id, ids[1000]);

    // 恰 1000 条时不截断
    let mut next_id = None;
    for _ in 0..200 {
        let entry = HistoryEntry {
            id: uuid::Uuid::new_v4(),
            reminder_id: uuid::Uuid::new_v4(),
            title: "喝水".to_string(),
            scheduled_at: dt(2026, 6, 1, 10, 0),
            status: HistoryStatus::Done,
            at: dt(2026, 6, 1, 10, 1),
        };
        next_id = Some(entry.id);
        store.append_history(&entry).unwrap();
    }
    store.truncate_history().unwrap();
    let kept = store.read_history(2000).unwrap();
    assert_eq!(kept.len(), 1000);
    assert_eq!(kept.last().unwrap().id, next_id.unwrap());
}

#[test]
fn read_history_skips_unparseable_lines() {
    use temporule_core::model::{HistoryEntry, HistoryStatus};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let reminder_id = uuid::Uuid::new_v4();

    let entry = |at: DateTime<Local>| HistoryEntry {
        id: uuid::Uuid::new_v4(),
        reminder_id,
        title: "提肛".to_string(),
        scheduled_at: at,
        status: HistoryStatus::Fired,
        at,
    };
    let first = entry(dt(2026, 6, 1, 9, 0));
    let second = entry(dt(2026, 6, 1, 10, 0));
    store.append_history(&first).unwrap();
    // 合法行之间夹一行损坏数据（半截 JSON）
    {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join("history.jsonl"))
            .unwrap();
        file.write_all("{ 这不是合法 JSON\n".as_bytes()).unwrap();
    }
    store.append_history(&second).unwrap();

    // 坏行被跳过，合法行照常返回（不会整体报错）
    assert_eq!(store.read_history(10).unwrap(), vec![first, second]);
}

#[test]
fn truncate_history_tolerates_unparseable_lines() {
    use temporule_core::model::{HistoryEntry, HistoryStatus};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();

    let entry = HistoryEntry {
        id: uuid::Uuid::new_v4(),
        reminder_id: uuid::Uuid::new_v4(),
        title: "提肛".to_string(),
        scheduled_at: dt(2026, 6, 1, 9, 0),
        status: HistoryStatus::Fired,
        at: dt(2026, 6, 1, 9, 1),
    };
    store.append_history(&entry).unwrap();
    {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join("history.jsonl"))
            .unwrap();
        file.write_all("坏行\n".as_bytes()).unwrap();
    }

    // 截断检查不再因单行坏数据整体失败（否则调度心跳停更）
    store.truncate_history().unwrap();
    assert_eq!(store.read_history(10).unwrap(), vec![entry]);
}

#[test]
fn snooze_add_due_remove_round_trip() {
    use temporule_core::model::SnoozeRecord;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let reminder_id = uuid::Uuid::new_v4();

    let early = SnoozeRecord {
        id: uuid::Uuid::new_v4(),
        reminder_id,
        due_at: dt(2026, 6, 1, 9, 10),
        created_at: dt(2026, 6, 1, 9, 0),
    };
    let late = SnoozeRecord {
        id: uuid::Uuid::new_v4(),
        reminder_id,
        due_at: dt(2026, 6, 1, 10, 30),
        created_at: dt(2026, 6, 1, 9, 0),
    };
    store.add_snooze(&early).unwrap();
    store.add_snooze(&late).unwrap();

    // 半开区间 (09:00, 10:30]：两条记录都命中
    assert_eq!(
        store
            .snoozes_due(dt(2026, 6, 1, 9, 0), dt(2026, 6, 1, 10, 30))
            .unwrap(),
        vec![early.clone(), late.clone()]
    );
    // (09:00, 09:10]：early 恰在终点 → 含；late 未到 → 不含
    assert_eq!(
        store
            .snoozes_due(dt(2026, 6, 1, 9, 0), dt(2026, 6, 1, 9, 10))
            .unwrap(),
        vec![early.clone()]
    );

    // consume-once：移除后不再命中
    store.remove_snooze(early.id).unwrap();
    assert_eq!(
        store
            .snoozes_due(dt(2026, 6, 1, 9, 0), dt(2026, 6, 1, 10, 30))
            .unwrap(),
        vec![late.clone()]
    );
    // 移除不存在的 id 是无害空操作
    store.remove_snooze(early.id).unwrap();
    assert_eq!(
        store
            .snoozes_due(dt(2026, 6, 1, 9, 0), dt(2026, 6, 1, 10, 30))
            .unwrap(),
        vec![late]
    );
}

#[test]
fn state_save_and_load_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();

    // 无状态文件 → None（调度器按无心跳启动）
    assert_eq!(store.load_state(), None);

    let tick = dt(2026, 6, 1, 12, 0);
    store.save_state(tick).unwrap();
    assert_eq!(store.load_state(), Some(tick));
}

#[test]
fn settings_load_returns_defaults_when_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.load_settings().unwrap(), AppSettings::default());
}

#[test]
fn settings_save_and_load_round_trip_preserves_values() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let settings = AppSettings {
        launch_at_login: true,
        default_sound: false,
        dashboard_hide_on_blur: false,
    };
    store.save_settings(&settings).unwrap();
    assert_eq!(store.load_settings().unwrap(), settings);
    assert!(dir.path().join("settings.json").exists());
}

#[test]
fn settings_corrupt_file_is_backed_up_and_defaults_returned() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    std::fs::write(dir.path().join("settings.json"), "{ 损坏").unwrap();

    assert_eq!(store.load_settings().unwrap(), AppSettings::default());
    // 损坏现场保留为 *.corrupt-* 备份，原文件不再存在
    assert!(!dir.path().join("settings.json").exists());
    assert!(std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e
            .file_name()
            .to_string_lossy()
            .starts_with("settings.json.corrupt-")));
}

#[test]
fn settings_save_leaves_no_tmp_files_behind() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store.save_settings(&AppSettings::default()).unwrap();
    assert!(std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .all(|e| !e.file_name().to_string_lossy().ends_with(".tmp")));
}

#[test]
fn write_invoke_then_take_returns_action_and_consumes_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();

    store.write_invoke("show_main").unwrap();
    // 文件已落盘
    assert!(dir.path().join("invoke.json").exists());

    let action = store.take_invoke().unwrap();
    assert_eq!(action.as_deref(), Some("show_main"));
    // consume-once：取走即删
    assert!(!dir.path().join("invoke.json").exists());
    assert_eq!(store.take_invoke().unwrap(), None);
}

#[test]
fn take_invoke_returns_none_when_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.take_invoke().unwrap(), None);
}

#[test]
fn take_invoke_tolerates_corrupt_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    std::fs::write(dir.path().join("invoke.json"), b"{not json").unwrap();
    assert_eq!(store.take_invoke().unwrap(), None);
    // 坏文件被消费删除
    assert!(!dir.path().join("invoke.json").exists());
}
