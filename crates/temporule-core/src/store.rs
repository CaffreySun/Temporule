//! 存储层：JSON 原子写 + jsonl 历史。
//!
//! 公开接缝：[`Store`] 的 `load` / `save` / `append_history` / `read_history` /
//! `truncate_history` / `add_snooze` / `snoozes_due` / `remove_snooze` / `load_state` / `save_state` /
//! `load_settings` / `save_settings`。
//! 所有写入走「唯一 tmp + fsync + rename」原子写，`reminders.json` 另有 rev 守卫防止过期快照覆盖。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{HistoryEntry, Reminder, SnoozeRecord};
use crate::settings::AppSettings;

/// 提醒数据文件名（`{ version, rev, reminders }`）。
pub const REMINDERS_FILE: &str = "reminders.json";
/// 历史记录文件名（jsonl 追加写）。
pub const HISTORY_FILE: &str = "history.jsonl";
/// 稍后记录文件名（`{ version, snoozes }`）。
pub const SNOOZES_FILE: &str = "snoozes.json";
/// 调度心跳状态文件名（`{ last_tick }`）。
pub const STATE_FILE: &str = "state.json";
/// 应用设置文件名（`{ version, settings }`）。
pub const SETTINGS_FILE: &str = "settings.json";

/// 存储文件格式版本。
const FORMAT_VERSION: u32 = 1;

/// 历史截断阈值：条数超过它时原子重写。
const HISTORY_TRUNCATE_THRESHOLD: usize = 1000;
/// 历史截断后保留的条数。
const HISTORY_KEEP: usize = 800;

/// 存储层错误，可穷举。
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// 文件读写失败。
    #[error("文件操作失败：{path}")]
    Io {
        /// 相关文件路径。
        path: PathBuf,
        /// 底层 IO 错误。
        #[source]
        source: std::io::Error,
    },
    /// JSON 解析/序列化失败。
    #[error("JSON 解析失败：{path}")]
    Json {
        /// 相关文件路径。
        path: PathBuf,
        /// 底层 serde 错误。
        #[source]
        source: serde_json::Error,
    },
    /// 保存冲突：待保存快照基于的 rev 与当前文件 rev 不一致。
    #[error("保存冲突：快照基于 rev {base_rev}，当前文件 rev 为 {current_rev}")]
    SaveConflict {
        /// 调用方持有的基线 rev。
        base_rev: u64,
        /// 文件中的当前 rev。
        current_rev: u64,
    },
    /// 待保存的提醒不满足模型不变量（与加载侧的强制校验对称）。
    #[error("提醒 {id} 不满足模型不变量：{reason}")]
    InvalidReminder {
        /// 不合法提醒的标识。
        id: uuid::Uuid,
        /// 校验失败原因。
        reason: crate::model::ValidationError,
    },
}

/// `reminders.json` 的一次性读入结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreData {
    /// 当前修订号（文件不存在时为 0）。
    pub rev: u64,
    /// 提醒列表。
    pub reminders: Vec<Reminder>,
}

/// `reminders.json` 的磁盘形态。
#[derive(Serialize, Deserialize)]
struct RemindersFile {
    /// 文件格式版本。
    version: u32,
    /// 修订号。
    rev: u64,
    /// 提醒列表。
    reminders: Vec<Reminder>,
}

/// `snoozes.json` 的磁盘形态。
#[derive(Serialize, Deserialize)]
struct SnoozesFile {
    /// 文件格式版本。
    version: u32,
    /// 稍后记录列表。
    snoozes: Vec<SnoozeRecord>,
}

/// `state.json` 的磁盘形态。
#[derive(Serialize, Deserialize)]
struct StateFile {
    /// 调度心跳时间（错过检测基准）。
    last_tick: chrono::DateTime<chrono::Local>,
}

/// `settings.json` 的磁盘形态。
#[derive(Serialize, Deserialize)]
struct SettingsFile {
    /// 文件格式版本。
    version: u32,
    /// 应用设置。
    settings: AppSettings,
}

/// 数据目录上的存储句柄。
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// 打开（必要时创建）数据目录上的存储句柄。
    pub fn open(dir: &Path) -> Result<Store, StoreError> {
        fs::create_dir_all(dir).map_err(|source| StoreError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        Ok(Store {
            dir: dir.to_path_buf(),
        })
    }

    /// 读取提醒数据。
    ///
    /// 文件不存在时返回 `rev = 0` 与空列表；JSON 损坏或含不满足模型不变量的提醒时，
    /// 原文件备份为 `*.corrupt-<ts>` 后返回默认（绝不 panic）。
    pub fn load(&self) -> Result<StoreData, StoreError> {
        let path = self.path(REMINDERS_FILE);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(StoreData {
                    rev: 0,
                    reminders: Vec::new(),
                })
            }
            Err(source) => return Err(StoreError::Io { path, source }),
        };
        match decode_reminders(&bytes) {
            Some(data) => Ok(data),
            None => {
                self.backup_corrupt(&path)?;
                Ok(StoreData {
                    rev: 0,
                    reminders: Vec::new(),
                })
            }
        }
    }

    /// 保存提醒列表并推进 rev，返回新 rev。
    ///
    /// 写协议：任一提醒不满足模型不变量时拒绝写入（[`StoreError::InvalidReminder`]）；
    /// 重读当前文件比对 `base_rev`（rev 守卫），不一致返回 [`StoreError::SaveConflict`]
    /// 且不写盘；一致则 `rev + 1` 原子落盘（唯一 tmp + fsync + rename）。
    pub fn save(&self, base_rev: u64, reminders: &[Reminder]) -> Result<u64, StoreError> {
        for reminder in reminders {
            if let Err(reason) = reminder.validate() {
                return Err(StoreError::InvalidReminder {
                    id: reminder.id,
                    reason,
                });
            }
        }
        let current = self.load()?;
        if current.rev != base_rev {
            return Err(StoreError::SaveConflict {
                base_rev,
                current_rev: current.rev,
            });
        }
        let new_rev = current.rev + 1;
        let file = RemindersFile {
            version: FORMAT_VERSION,
            rev: new_rev,
            reminders: reminders.to_vec(),
        };
        let path = self.path(REMINDERS_FILE);
        let bytes = serde_json::to_vec_pretty(&file).map_err(|source| StoreError::Json {
            path: path.clone(),
            source,
        })?;
        self.atomic_write(&path, &bytes)?;
        Ok(new_rev)
    }

    /// 追加一条历史记录（`history.jsonl`，append-only）。
    ///
    /// 写入走追加 + fsync；截断只经 [`Store::truncate_history`]。
    pub fn append_history(&self, entry: &HistoryEntry) -> Result<(), StoreError> {
        let path = self.path(HISTORY_FILE);
        let mut line = serde_json::to_string(entry).map_err(|source| StoreError::Json {
            path: path.clone(),
            source,
        })?;
        line.push('\n');
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| StoreError::Io {
                path: path.clone(),
                source,
            })?;
        file.write_all(line.as_bytes())
            .map_err(|source| StoreError::Io {
                path: path.clone(),
                source,
            })?;
        file.sync_all()
            .map_err(|source| StoreError::Io { path, source })
    }

    /// 读取最近 `limit` 条历史记录（按时间序，文件缺失返回空）。
    pub fn read_history(&self, limit: usize) -> Result<Vec<HistoryEntry>, StoreError> {
        let path = self.path(HISTORY_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(StoreError::Io { path, source }),
        };
        let mut entries = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: HistoryEntry =
                serde_json::from_str(line).map_err(|source| StoreError::Json {
                    path: path.clone(),
                    source,
                })?;
            entries.push(entry);
        }
        let start = entries.len().saturating_sub(limit);
        Ok(entries.split_off(start))
    }

    /// 历史截断：条数超过 1000 时原子重写，保留最近 800 条；否则不动文件。
    pub fn truncate_history(&self) -> Result<(), StoreError> {
        let entries = self.read_history(usize::MAX)?;
        if entries.len() <= HISTORY_TRUNCATE_THRESHOLD {
            return Ok(());
        }
        let keep = &entries[entries.len() - HISTORY_KEEP..];
        let mut bytes = Vec::new();
        for entry in keep {
            bytes.extend_from_slice(
                serde_json::to_vec(entry)
                    .map_err(|source| StoreError::Json {
                        path: self.path(HISTORY_FILE),
                        source,
                    })?
                    .as_slice(),
            );
            bytes.push(b'\n');
        }
        self.atomic_write(&self.path(HISTORY_FILE), &bytes)
    }

    /// 新增一条一次性稍后记录（`snoozes.json`）。
    pub fn add_snooze(&self, record: &SnoozeRecord) -> Result<(), StoreError> {
        let mut snoozes = self.read_snoozes()?;
        snoozes.push(record.clone());
        self.write_snoozes(&snoozes)
    }

    /// 返回到点时刻落在半开区间 `(from_exclusive, to_inclusive]` 的稍后记录。
    pub fn snoozes_due(
        &self,
        from_exclusive: chrono::DateTime<chrono::Local>,
        to_inclusive: chrono::DateTime<chrono::Local>,
    ) -> Result<Vec<SnoozeRecord>, StoreError> {
        Ok(self
            .read_snoozes()?
            .into_iter()
            .filter(|record| record.due_at > from_exclusive && record.due_at <= to_inclusive)
            .collect())
    }

    /// 移除一条稍后记录（consume-once）；id 不存在时是无害空操作。
    pub fn remove_snooze(&self, id: uuid::Uuid) -> Result<(), StoreError> {
        let mut snoozes = self.read_snoozes()?;
        let before = snoozes.len();
        snoozes.retain(|record| record.id != id);
        if snoozes.len() == before {
            return Ok(());
        }
        self.write_snoozes(&snoozes)
    }

    /// 读取全部稍后记录；文件缺失返回空，JSON 损坏上抛 [`StoreError::Json`]（不静默吞掉）。
    fn read_snoozes(&self) -> Result<Vec<SnoozeRecord>, StoreError> {
        let path = self.path(SNOOZES_FILE);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(StoreError::Io { path, source }),
        };
        let file: SnoozesFile =
            serde_json::from_slice(&bytes).map_err(|source| StoreError::Json { path, source })?;
        Ok(file.snoozes)
    }

    /// 原子写回全部稍后记录。
    fn write_snoozes(&self, snoozes: &[SnoozeRecord]) -> Result<(), StoreError> {
        let file = SnoozesFile {
            version: FORMAT_VERSION,
            snoozes: snoozes.to_vec(),
        };
        let path = self.path(SNOOZES_FILE);
        let bytes = serde_json::to_vec_pretty(&file).map_err(|source| StoreError::Json {
            path: path.clone(),
            source,
        })?;
        self.atomic_write(&path, &bytes)
    }

    /// 读取调度心跳时间（`state.json` 的 `last_tick`）。
    ///
    /// 文件缺失或损坏返回 `None`（按无心跳启动，不 panic）。
    pub fn load_state(&self) -> Option<chrono::DateTime<chrono::Local>> {
        let path = self.path(STATE_FILE);
        let text = fs::read_to_string(&path).ok()?;
        let file: StateFile = serde_json::from_str(&text).ok()?;
        Some(file.last_tick)
    }

    /// 原子写入调度心跳时间。
    pub fn save_state(&self, tick: chrono::DateTime<chrono::Local>) -> Result<(), StoreError> {
        let file = StateFile { last_tick: tick };
        let path = self.path(STATE_FILE);
        let bytes = serde_json::to_vec_pretty(&file).map_err(|source| StoreError::Json {
            path: path.clone(),
            source,
        })?;
        self.atomic_write(&path, &bytes)
    }

    /// 读取应用设置（`settings.json`）。
    ///
    /// 文件缺失或损坏时返回 [`AppSettings::default`]（损坏文件备份为 `*.corrupt-<ts>`，
    /// 与提醒数据的宽容读取对称，绝不 panic）。
    pub fn load_settings(&self) -> Result<AppSettings, StoreError> {
        let path = self.path(SETTINGS_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AppSettings::default())
            }
            Err(source) => return Err(StoreError::Io { path, source }),
        };
        match serde_json::from_str::<SettingsFile>(&text) {
            Ok(file) => Ok(file.settings),
            Err(_) => {
                self.backup_corrupt(&path)?;
                Ok(AppSettings::default())
            }
        }
    }

    /// 原子写入应用设置（`settings.json`，唯一 tmp + fsync + rename，与其余数据文件同规则）。
    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), StoreError> {
        let file = SettingsFile {
            version: FORMAT_VERSION,
            settings: *settings,
        };
        let path = self.path(SETTINGS_FILE);
        let bytes = serde_json::to_vec_pretty(&file).map_err(|source| StoreError::Json {
            path: path.clone(),
            source,
        })?;
        self.atomic_write(&path, &bytes)
    }

    /// 拼接数据目录下的文件路径。
    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// 原子写：唯一 tmp 名（pid + uuid）→ write + fsync → rename → 目录 fsync。
    ///
    /// 禁止共享固定 tmp 名（并发写会撕裂文件）；fsync 在 rename 前，崩溃不留空文件。
    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        let tmp_path = self.dir.join(format!(
            "{file_name}.{}.{}.tmp",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let result = self.write_via_tmp(&tmp_path, path, bytes);
        if result.is_err() {
            // 失败清理临时文件，避免残留
            let _ = fs::remove_file(&tmp_path);
        }
        result
    }

    /// 经 tmp 文件完成写入与改名。
    fn write_via_tmp(&self, tmp_path: &Path, path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
        let mut file = fs::File::create(tmp_path).map_err(|source| StoreError::Io {
            path: tmp_path.to_path_buf(),
            source,
        })?;
        file.write_all(bytes).map_err(|source| StoreError::Io {
            path: tmp_path.to_path_buf(),
            source,
        })?;
        file.sync_all().map_err(|source| StoreError::Io {
            path: tmp_path.to_path_buf(),
            source,
        })?;
        drop(file);
        fs::rename(tmp_path, path).map_err(|source| StoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        // 目录 fsync 保证 rename 落盘；Windows 不支持目录句柄 fsync，按尽力而为处理
        if let Ok(dir) = fs::File::open(&self.dir) {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    /// 把损坏的数据文件改名为 `*.corrupt-<ts>` 备份，保留现场供人工排查。
    fn backup_corrupt(&self, path: &Path) -> Result<(), StoreError> {
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        let backup_path = self.dir.join(format!(
            "{file_name}.corrupt-{}",
            chrono::Local::now().timestamp_millis()
        ));
        fs::rename(path, &backup_path).map_err(|source| StoreError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

/// 解析 `reminders.json` 内容；JSON 损坏或任一提醒不满足模型不变量时返回 `None`
///（模型不变量在存储加载处强制，见 `Reminder::validate`）。
fn decode_reminders(bytes: &[u8]) -> Option<StoreData> {
    let file: RemindersFile = serde_json::from_slice(bytes).ok()?;
    if file.reminders.iter().any(|r| r.validate().is_err()) {
        return None;
    }
    Some(StoreData {
        rev: file.rev,
        reminders: file.reminders,
    })
}
