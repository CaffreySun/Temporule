/**
 * Tauri command 调用封装与前端数据类型（字段名与 Rust 命令层的线格式一一对应）。
 * UI 文案一律中文；时间文本格式：表单 HH:MM，时间戳 RFC3339。
 */

/** 提醒列表行（commands::ReminderView）。 */
export interface ReminderView {
  /** 提醒标识。 */
  id: string;
  /** 标题。 */
  title: string;
  /** 备注。 */
  note: string | null;
  /** 中文排程摘要。 */
  schedule_desc: string;
  /** 是否启用。 */
  enabled: boolean;
  /** 下次触发时刻（RFC3339；null 表示无，显示「—」）。 */
  next_fire: string | null;
  /** 错过是否补发。 */
  catch_up: boolean;
  /** 触达方式：notification | popup。 */
  delivery_type: string;
}

/** 新增/编辑表单载荷（commands::ReminderPayload）。 */
export interface ReminderPayload {
  /** 标题。 */
  title: string;
  /** 备注。 */
  note: string | null;
  /** 重复规则：daily | weekly。 */
  repeat: string;
  /** 每周重复的星期（mon…sun）。 */
  weekdays: string[];
  /** 排程模式：every（间隔+时间窗）| at（固定时刻）。 */
  schedule_mode: string;
  /** 间隔分钟数。 */
  interval_minutes: number;
  /** 时间窗起点 HH:MM。 */
  window_start: string;
  /** 时间窗终点 HH:MM。 */
  window_end: string;
  /** 固定时刻列表 HH:MM。 */
  times: string[];
  /** 触达方式：notification | popup。 */
  delivery: string;
  /** 系统通知是否带声音。 */
  sound: boolean;
  /** 弹窗行为：auto（自动消失）| force（强制处理）。 */
  popup_mode: string;
  /** 弹窗自动消失秒数。 */
  auto_dismiss_seconds: number;
  /** 「稍后」分钟数。 */
  snooze_minutes: number;
  /** 错过是否补发。 */
  catch_up: boolean;
}

/** 编辑表单预填数据（commands::ReminderDetail）。 */
export interface ReminderDetail extends ReminderPayload {
  /** 提醒标识。 */
  id: string;
  /** 是否启用。 */
  enabled: boolean;
}

/** 历史条目（core::model::HistoryEntry）。 */
export interface HistoryEntryView {
  /** 条目标识。 */
  id: string;
  /** 所属提醒标识。 */
  reminder_id: string;
  /** 触发时的标题快照。 */
  title: string;
  /** 计划触发时刻（RFC3339）。 */
  scheduled_at: string;
  /** 处理结果：fired | done | dismissed | snoozed | missed。 */
  status: string;
  /** 记录时刻（RFC3339）。 */
  at: string;
}

/** 应用设置（core::settings::AppSettings）。 */
export interface AppSettingsView {
  /** 开机自启。 */
  launch_at_login: boolean;
  /** 默认声音。 */
  default_sound: boolean;
  /** 仪表盘失焦自动隐藏。 */
  dashboard_hide_on_blur: boolean;
}

const invoke = window.__TAURI__.core.invoke;

/** 错误 → 可读文案（command 错误为字符串）。 */
export function formatError(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** 是否为 SaveConflict（command 层固定文案含「已被外部修改」）。 */
export function isSaveConflictError(err: unknown): boolean {
  return formatError(err).includes("已被外部修改");
}

/** 历史状态 → 中文标签。 */
export function historyStatusLabel(status: string): string {
  switch (status) {
    case "fired":
      return "已触发";
    case "done":
      return "已完成";
    case "dismissed":
      return "已忽略";
    case "snoozed":
      return "已稍后";
    case "missed":
      return "已错过";
    default:
      return status;
  }
}

/** RFC3339 → 本地「MM-DD HH:MM」；null 显示「—」。 */
export function formatDateTime(iso: string | null): string {
  if (iso === null) {
    return "—";
  }
  const date = new Date(iso);
  const pad = (value: number): string => String(value).padStart(2, "0");
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** 提醒全部。 */
export function listReminders(): Promise<ReminderView[]> {
  return invoke<ReminderView[]>("list");
}

/** 提醒完整字段（编辑表单预填）。 */
export function getReminder(id: string): Promise<ReminderDetail> {
  return invoke<ReminderDetail>("get", { id });
}

/** 新增提醒。 */
export function addReminder(payload: ReminderPayload): Promise<void> {
  return invoke<void>("add", { payload });
}

/** 编辑提醒。 */
export function updateReminder(id: string, payload: ReminderPayload): Promise<void> {
  return invoke<void>("update", { id, payload });
}

/** 删除提醒。 */
export function removeReminder(id: string): Promise<void> {
  return invoke<void>("remove", { id });
}

/** 启用/禁用提醒。 */
export function toggleReminder(id: string, enabled: boolean): Promise<void> {
  return invoke<void>("toggle", { id, enabled });
}

/** 历史记录（最近 limit 条，时间序）。 */
export function fetchHistory(limit: number): Promise<HistoryEntryView[]> {
  return invoke<HistoryEntryView[]>("history", { limit });
}

/** 读取应用设置。 */
export function fetchSettings(): Promise<AppSettingsView> {
  return invoke<AppSettingsView>("settings");
}

/** 保存应用设置。 */
export function saveAppSettings(settings: AppSettingsView): Promise<void> {
  return invoke<void>("save_settings", { settings });
}

/** 创建/聚焦主窗口。 */
export function showMainWindow(): Promise<void> {
  return invoke<void>("show_main_window");
}

/** 数据目录路径。 */
export function fetchDataDir(): Promise<string> {
  return invoke<string>("data_dir");
}

/** 在访达中打开数据目录。 */
export function openDataDir(): Promise<void> {
  return invoke<void>("open_data_dir");
}

/** 订阅「数据已变更」事件（外部修改提醒数据后触发）。 */
export function onStoreChanged(handler: () => void): void {
  void window.__TAURI__.event
    .listen<null>("store-changed", () => handler())
    .catch(() => {
      // 事件订阅失败不阻塞 UI，列表可手动刷新
    });
}
