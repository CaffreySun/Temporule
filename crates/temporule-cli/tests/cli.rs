//! Temporule CLI 进程级集成测试（接缝 #3：assert_cmd）。
//!
//! 每个用例用 `TEMPORULE_DATA_DIR` 指向独立临时目录，验证 stdout、退出码与数据文件副作用。

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;

/// 构造指向独立数据目录的 CLI 命令。
fn temporule(data: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("temporule").unwrap();
    cmd.env("TEMPORULE_DATA_DIR", data.path());
    cmd
}

/// 读取数据目录下的文件文本。
fn read_data(data: &TempDir, name: &str) -> String {
    fs::read_to_string(data.path().join(name)).unwrap()
}

/// `add` 成功后写入 `reminders.json`，`list` 表格含标题与排程摘要。
#[test]
fn add_writes_reminder_and_list_shows_title_and_summary() {
    let data = TempDir::new().unwrap();
    temporule(&data)
        .args([
            "add",
            "--title",
            "喝水",
            "--daily",
            "--every",
            "2h",
            "--window",
            "9:00-18:00",
            "--notification",
        ])
        .assert()
        .success();

    // 副作用：reminders.json 存在且含标题
    assert!(Path::new(&data.path().join("reminders.json")).exists());
    assert!(read_data(&data, "reminders.json").contains("喝水"));

    // list 表格含标题与描述式排程摘要（字面期望，取自规格示例）
    let assert = temporule(&data).arg("list").assert().success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(out.contains("喝水"), "list 应含标题，实际输出：{out}");
    assert!(
        out.contains("每天 09:00–18:00 每 2 小时"),
        "list 应含排程摘要，实际输出：{out}"
    );
}

/// `list --json` 输出完整字段数组：id/title/schedule/enabled/next_fire 缺一不可。
#[test]
fn list_json_contains_complete_fields() {
    let data = TempDir::new().unwrap();
    temporule(&data)
        .args([
            "add",
            "--title",
            "站会",
            "--daily",
            "--at",
            "9:00,14:30",
            "--popup",
        ])
        .assert()
        .success();

    let assert = temporule(&data).args(["list", "--json"]).assert().success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let items: serde_json::Value = serde_json::from_str(&out).expect("list --json 应是合法 JSON");
    let item = &items[0];
    for key in ["id", "title", "schedule", "enabled", "next_fire"] {
        assert!(
            item.get(key).is_some(),
            "JSON 应含字段 {key}，实际输出：{out}"
        );
    }
    assert_eq!(item["title"], "站会");
    assert_eq!(item["enabled"], true);
}

/// 读取 `reminders.json` 的第一条提醒 id。
fn first_id(data: &TempDir) -> String {
    let raw = read_data(data, "reminders.json");
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    json["reminders"][0]["id"].as_str().unwrap().to_string()
}

/// `list --json` 的全部条目。
fn list_json(data: &TempDir) -> serde_json::Value {
    let assert = temporule(data).args(["list", "--json"]).assert().success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    serde_json::from_str(&out).unwrap()
}

/// `edit` 部分更新：改标题与间隔后，JSON 与排程摘要都反映新值。
#[test]
fn edit_updates_title_and_interval() {
    let data = TempDir::new().unwrap();
    temporule(&data)
        .args([
            "add",
            "--title",
            "喝水",
            "--daily",
            "--every",
            "2h",
            "--window",
            "9:00-18:00",
            "--notification",
        ])
        .assert()
        .success();
    let id = first_id(&data);

    temporule(&data)
        .args([
            "edit",
            &id,
            "--title",
            "补水",
            "--every",
            "30m",
            "--window",
            "9:00-18:00",
        ])
        .assert()
        .success();

    let items = list_json(&data);
    assert_eq!(items[0]["title"], "补水");
    // 排程为外部标签枚举（core 序列化格式）：schedule.every.interval_minutes
    assert_eq!(items[0]["schedule"]["every"]["interval_minutes"], 30);
    assert_eq!(items[0]["schedule_summary"], "每天 09:00–18:00 每 30 分钟");
}

/// `disable` / `enable`：JSON 中 `enabled` 状态翻转，表格「启用」列同步。
#[test]
fn disable_and_enable_flip_enabled_state() {
    let data = TempDir::new().unwrap();
    temporule(&data)
        .args([
            "add",
            "--title",
            "拉伸",
            "--daily",
            "--at",
            "10:00",
            "--notification",
        ])
        .assert()
        .success();
    let id = first_id(&data);

    temporule(&data).args(["disable", &id]).assert().success();
    let items = list_json(&data);
    assert_eq!(items[0]["enabled"], false);
    let assert = temporule(&data).arg("list").assert().success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(out.contains("否"), "停用后表格「启用」列应为否：{out}");

    temporule(&data).args(["enable", &id]).assert().success();
    let items = list_json(&data);
    assert_eq!(items[0]["enabled"], true);
}

/// `snooze <id> <10m|1h>` 写入 `snoozes.json`，记录含 `due_at`。
#[test]
fn snooze_writes_snoozes_file_with_due_at() {
    let data = TempDir::new().unwrap();
    temporule(&data)
        .args([
            "add", "--title", "复盘", "--daily", "--at", "18:00", "--popup",
        ])
        .assert()
        .success();
    let id = first_id(&data);

    temporule(&data)
        .args(["snooze", &id, "10m"])
        .assert()
        .success();

    let raw = read_data(&data, "snoozes.json");
    let json: serde_json::Value = serde_json::from_str(&raw).expect("snoozes.json 应是合法 JSON");
    let record = &json["snoozes"][0];
    assert!(
        record.get("due_at").is_some(),
        "snooze 记录应含 due_at，实际：{raw}"
    );
    assert_eq!(record["reminder_id"], id);
}

/// `history` 从空到非空：snooze 动作记入历史（新→旧），表格含标题与处理结果。
#[test]
fn history_goes_from_empty_to_non_empty_after_snooze() {
    let data = TempDir::new().unwrap();
    temporule(&data)
        .args([
            "add", "--title", "复盘", "--daily", "--at", "18:00", "--popup",
        ])
        .assert()
        .success();
    let id = first_id(&data);

    // 初始历史为空
    let assert = temporule(&data).arg("history").assert().success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(!out.contains("复盘"), "初始历史应为空，实际输出：{out}");

    temporule(&data)
        .args(["snooze", &id, "1h"])
        .assert()
        .success();

    // snooze 后出现「已稍后」记录
    let assert = temporule(&data).arg("history").assert().success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(out.contains("复盘"), "历史应含标题，实际输出：{out}");
    assert!(out.contains("已稍后"), "历史应含处理结果，实际输出：{out}");
}

/// `remove` 无 `-y` 且 stdin 非 TTY：拒绝删除并退出码 2，提醒仍在。
#[test]
fn remove_without_yes_is_rejected_when_not_a_tty() {
    let data = TempDir::new().unwrap();
    temporule(&data)
        .args([
            "add",
            "--title",
            "临时任务",
            "--daily",
            "--at",
            "8:00",
            "--notification",
        ])
        .assert()
        .success();
    let id = first_id(&data);

    // assert_cmd 的 stdin 非 TTY
    let assert = temporule(&data)
        .args(["remove", &id])
        .assert()
        .failure()
        .code(2);
    let err = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        err.contains("-y"),
        "拒绝信息应提示 -y/--yes，实际 stderr：{err}"
    );

    // 拒绝后提醒仍在
    let items = list_json(&data);
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["title"], "临时任务");
}

/// `remove -y` 直接删除，`list` 为空。
#[test]
fn remove_with_yes_deletes_and_list_is_empty() {
    let data = TempDir::new().unwrap();
    temporule(&data)
        .args([
            "add",
            "--title",
            "临时任务",
            "--daily",
            "--at",
            "8:00",
            "--notification",
        ])
        .assert()
        .success();
    let id = first_id(&data);

    temporule(&data)
        .args(["remove", &id, "-y"])
        .assert()
        .success();

    let items = list_json(&data);
    assert_eq!(items.as_array().unwrap().len(), 0, "删除后应无提醒");
    let assert = temporule(&data).arg("list").assert().success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(!out.contains("临时任务"), "list 不应再含标题：{out}");
}

/// `--force` 与 `--auto-dismiss` 同现：clap 拒绝，退出码 2。
#[test]
fn force_and_auto_dismiss_are_mutually_exclusive() {
    let data = TempDir::new().unwrap();
    let assert = temporule(&data)
        .args([
            "add",
            "--title",
            "冲突用例",
            "--daily",
            "--at",
            "9:00",
            "--popup",
            "--force",
            "--auto-dismiss",
            "30",
        ])
        .assert()
        .failure()
        .code(2);
    let err = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        err.contains("auto-dismiss") && err.contains("force"),
        "报错应同时点名两个互斥 flag，实际 stderr：{err}"
    );
}

/// `edit --notification` 拒绝 `--force` / `--auto-dismiss`（弹窗专属 flag）：退出码 2，不落任何变更。
#[test]
fn edit_notification_rejects_popup_only_flags() {
    let data = TempDir::new().unwrap();
    temporule(&data)
        .args([
            "add", "--title", "喝水", "--daily", "--at", "9:00", "--popup", "--force",
        ])
        .assert()
        .success();
    let id = first_id(&data);

    for extra in [vec!["--force"], vec!["--auto-dismiss", "10"]] {
        let assert = temporule(&data)
            .args(["edit", &id, "--notification"])
            .args(&extra)
            .assert()
            .failure()
            .code(2);
        let err = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
        assert!(
            err.contains("仅适用于弹窗"),
            "报错应指明弹窗专属 flag，实际 stderr：{err}"
        );
    }

    // 拒绝后提醒未被改动（仍是强制处理弹窗，未静默切成通知）
    let items = list_json(&data);
    assert_eq!(
        items[0]["delivery"]["popup"]["alert"],
        serde_json::json!("require_action")
    );
}

/// 裸 `add`（无任何 flags）：clap 报缺必填项，退出码 2，不写任何数据。
#[test]
fn bare_add_fails_with_usage_error() {
    let data = TempDir::new().unwrap();
    let assert = temporule(&data).arg("add").assert().failure().code(2);
    let err = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        err.contains("title") && err.contains("daily"),
        "报错应点名必填项，实际 stderr：{err}"
    );
    assert!(
        !data.path().join("reminders.json").exists(),
        "缺必填项不应写数据文件"
    );
}

/// 未知 id：报错非零（退出码 2），stderr 中文说明，不写数据。
#[test]
fn unknown_id_fails_with_nonzero_exit() {
    let data = TempDir::new().unwrap();
    temporule(&data)
        .args([
            "add",
            "--title",
            "已知提醒",
            "--daily",
            "--at",
            "9:00",
            "--notification",
        ])
        .assert()
        .success();

    for args in [
        vec!["edit", "00000000", "--title", "改名"],
        vec!["enable", "00000000"],
        vec!["snooze", "00000000", "10m"],
        vec!["remove", "00000000", "-y"],
    ] {
        let assert = temporule(&data).args(&args).assert().failure().code(2);
        let err = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
        assert!(
            err.contains("找不到提醒"),
            "未知 id 应报「找不到提醒」，实际 stderr：{err}"
        );
    }
}

/// `--every` 支持 `30m` / `2h` 语法：JSON 间隔分钟数与排程摘要都正确。
#[test]
fn every_duration_syntax_is_parsed() {
    for (duration, minutes, summary) in [
        ("30m", 30, "每天 09:00–18:00 每 30 分钟"),
        ("2h", 120, "每天 09:00–18:00 每 2 小时"),
    ] {
        let data = TempDir::new().unwrap();
        temporule(&data)
            .args([
                "add",
                "--title",
                "喝水",
                "--daily",
                "--every",
                duration,
                "--window",
                "9:00-18:00",
                "--notification",
            ])
            .assert()
            .success();
        let items = list_json(&data);
        assert_eq!(items[0]["schedule"]["every"]["interval_minutes"], minutes);
        assert_eq!(items[0]["schedule_summary"], summary);
    }
}

/// 模型不变量校验失败（间隔 < 1）：stderr 中文错误 + 退出码 2，不写数据。
#[test]
fn invalid_interval_is_rejected_with_chinese_error() {
    let data = TempDir::new().unwrap();
    let assert = temporule(&data)
        .args([
            "add",
            "--title",
            "非法间隔",
            "--daily",
            "--every",
            "0m",
            "--window",
            "9:00-18:00",
            "--notification",
        ])
        .assert()
        .failure()
        .code(2);
    let err = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        err.contains("间隔分钟数必须 ≥ 1"),
        "应输出中文校验错误，实际 stderr：{err}"
    );
    assert!(
        !data.path().join("reminders.json").exists(),
        "校验失败不应写数据文件"
    );
}

/// flags 全量模式其余变体正确落库：weekly / at / notification+sound / note / snooze / catch-up。
#[test]
fn full_flag_variants_land_in_json() {
    let data = TempDir::new().unwrap();
    temporule(&data)
        .args([
            "add",
            "--title",
            "周会",
            "--note",
            "带上周报",
            "--weekly",
            "mon,wed",
            "--at",
            "9:00,14:30",
            "--notification",
            "--sound",
            "--snooze",
            "5",
            "--catch-up",
        ])
        .assert()
        .success();

    let items = list_json(&data);
    assert_eq!(items[0]["note"], "带上周报");
    assert_eq!(
        items[0]["repeat"]["weekly"],
        serde_json::json!(["mon", "wed"])
    );
    assert_eq!(
        items[0]["schedule"]["at"]["times"],
        serde_json::json!(["09:00:00", "14:30:00"])
    );
    assert_eq!(items[0]["delivery"]["notification"]["sound"], true);
    assert_eq!(items[0]["snooze_minutes"], 5);
    assert_eq!(items[0]["catch_up"], true);
}

/// 弹窗子配置：裸 `--popup` = 30 秒自动消失，`--popup --force` = 必须手动处理。
#[test]
fn popup_variants_land_in_json() {
    let data = TempDir::new().unwrap();
    temporule(&data)
        .args([
            "add",
            "--title",
            "默认弹窗",
            "--daily",
            "--at",
            "9:00",
            "--popup",
        ])
        .assert()
        .success();
    temporule(&data)
        .args([
            "add",
            "--title",
            "强制处理",
            "--daily",
            "--at",
            "10:00",
            "--popup",
            "--force",
        ])
        .assert()
        .success();

    let items = list_json(&data);
    assert_eq!(
        items[0]["delivery"]["popup"]["alert"],
        serde_json::json!({ "auto_dismiss": { "seconds": 30 } })
    );
    // unit 变体在外部标签序列化下是裸字符串
    assert_eq!(
        items[1]["delivery"]["popup"]["alert"],
        serde_json::json!("require_action")
    );
}

/// `history --json`：输出完整字段，顺序为新→旧。
#[test]
fn history_json_is_newest_first() {
    let data = TempDir::new().unwrap();
    temporule(&data)
        .args([
            "add", "--title", "复盘", "--daily", "--at", "18:00", "--popup",
        ])
        .assert()
        .success();
    let id = first_id(&data);

    temporule(&data)
        .args(["snooze", &id, "10m"])
        .assert()
        .success();
    temporule(&data)
        .args(["snooze", &id, "1h"])
        .assert()
        .success();

    let assert = temporule(&data)
        .args(["history", "--json", "--limit", "10"])
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let entries: serde_json::Value = serde_json::from_str(&out).unwrap();
    let entries = entries.as_array().unwrap();
    assert_eq!(entries.len(), 2);
    for key in ["id", "reminder_id", "title", "scheduled_at", "status", "at"] {
        assert!(entries[0].get(key).is_some(), "历史 JSON 应含字段 {key}");
    }
    // 新→旧：最新一条（1h）的 due 晚于 10m 那条 → created 顺序靠后排前
    assert!(entries[0]["at"].as_str() >= entries[1]["at"].as_str());
    assert_eq!(entries[0]["status"], "snoozed");
}

/// `open` 写 invoke.json（文件落点 IPC），TEMPORULE_NO_LAUNCH 下不启动 App。
#[test]
fn open_writes_invoke_file_without_launching() {
    let data = TempDir::new().unwrap();
    temporule(&data)
        .env("TEMPORULE_NO_LAUNCH", "1")
        .arg("open")
        .assert()
        .success();
    let invoke = read_data(&data, "invoke.json");
    assert_eq!(invoke.trim(), r#"{"action":"show_dashboard"}"#);
}
