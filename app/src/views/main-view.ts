/**
 * 主窗口视图（v2）：顶部栏 = Tab（当前项 text + 底部 2px accent 指示条，其余 text-dim）
 * + 右上 ghost「+ 新增」。提醒页 = 行列表（v2 Row，同仪表盘）；历史页 = 时间线
 * （日期分组头 + 行：时间 tabular + 状态点 + 标题）；设置页 = 分组（分组标题 11/600 +
 * 行式开关）。
 * 组件约定：组件 = 一个返回 HTMLElement 的函数。
 */
import {
  fetchHistory,
  fetchDataDir,
  fetchSettings,
  formatError,
  historyStatusLabel,
  listReminders,
  onStoreChanged,
  openDataDir,
  requestNotificationPermission,
  saveAppSettings,
  type AppSettingsView,
  type ReminderDetail,
} from "../api";
import { alertDialog } from "../components/dialog";
import { reminderForm } from "../components/reminder-form";
import { reminderRow } from "../components/reminder-row";

/** 历史展示上限。 */
const HISTORY_LIMIT = 200;

/** 提醒 Tab 句柄：供顶部「+ 新增」安全触发（表单挂载中为空操作，不吞未保存输入）。 */
interface RemindersTabHandle {
  /** 打开新增表单。 */
  openNew: () => void;
}

/** Tab 配置：标题 + 激活时的重建函数（由 mainView 注入）。 */
interface TabSpec {
  /** Tab 标识。 */
  id: string;
  /** Tab 中文标题。 */
  title: string;
  /** 激活该 Tab 时重建内容；提醒 Tab 返回句柄，其余返回 null。 */
  refresh: (pane: HTMLElement) => RemindersTabHandle | null;
}

/** 一个 Tab 的完整句柄。 */
interface TabHandle {
  spec: TabSpec;
  button: HTMLButtonElement;
  pane: HTMLElement;
}

/** 提醒 Tab：行列表 ↔ 表单 双态；表单挂载期间不重build（不吞未保存输入）。 */
function renderRemindersTab(pane: HTMLElement): RemindersTabHandle {
  const showForm = (initial: ReminderDetail | undefined): void => {
    pane.replaceChildren(
      reminderForm({
        initial,
        onFinished: showTable,
        onCancel: showTable,
      }),
    );
  };

  const showTable = (): void => {
    pane.replaceChildren();
    const listBox = document.createElement("div");
    listBox.className = "row-list";
    const loading = document.createElement("p");
    loading.className = "hint";
    loading.textContent = "加载中…";
    listBox.append(loading);
    pane.append(listBox);

    listReminders()
      .then((reminders) => {
        listBox.replaceChildren();
        if (reminders.length === 0) {
          const empty = document.createElement("p");
          empty.className = "hint";
          empty.textContent = "暂无提醒，点击「+ 新增」创建第一条。";
          listBox.append(empty);
          return;
        }
        for (const reminder of reminders) {
          listBox.append(
            reminderRow({
              reminder,
              onEdit: (detail) => showForm(detail),
              onChanged: showTable,
            }),
          );
        }
      })
      .catch((err: unknown) => {
        listBox.replaceChildren();
        const failed = document.createElement("p");
        failed.className = "field-error";
        failed.textContent = `加载失败：${formatError(err)}`;
        listBox.append(failed);
      });
  };

  // openNew 调用时实时检查表单挂起：表单挂载中不吞未保存输入
  const openNew = (): void => {
    if (pane.querySelector("form.reminder-form") === null) {
      showForm(undefined);
    }
  };

  // 表单挂载期间不重建（store-changed / 切 Tab 均不吞未保存输入；保存/取消收尾时重建列表）
  if (pane.querySelector("form.reminder-form") === null) {
    showTable();
  }
  return { openNew };
}

/** RFC3339 → 「HH:MM」（本地时区）。 */
function formatTime(iso: string): string {
  const date = new Date(iso);
  const pad = (value: number): string => String(value).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** RFC3339 → 「MM-DD 周X」（本地时区，日期分组头）。 */
function formatDateGroup(iso: string): string {
  const date = new Date(iso);
  const pad = (value: number): string => String(value).padStart(2, "0");
  const weekdays = "日一二三四五六";
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} 周${weekdays[date.getDay()]}`;
}

/** 历史 Tab：时间线（日期分组头 + 行：时间 tabular + 状态点 + 标题），新→旧。 */
function renderHistoryTab(pane: HTMLElement): null {
  pane.replaceChildren();
  const loading = document.createElement("p");
  loading.className = "hint";
  loading.textContent = "加载中…";
  pane.append(loading);

  fetchHistory(HISTORY_LIMIT)
    .then((entries) => {
      pane.replaceChildren();
      if (entries.length === 0) {
        const empty = document.createElement("p");
        empty.className = "hint";
        empty.textContent = "暂无历史记录。";
        pane.append(empty);
        return;
      }
      const timeline = document.createElement("div");
      timeline.className = "timeline";
      let lastGroup = "";
      for (const entry of [...entries].reverse()) {
        // 日期分组头（11/600 text-dim + 字距）
        const group = formatDateGroup(entry.at);
        if (group !== lastGroup) {
          lastGroup = group;
          const head = document.createElement("p");
          head.className = "group-title";
          head.textContent = group;
          timeline.append(head);
        }
        // 行：时间（tabular, text-dim）+ 状态点（语义色）+ 标题，单行省略
        const row = document.createElement("div");
        row.className = "timeline-row";
        const time = document.createElement("span");
        time.className = "timeline-time";
        time.textContent = formatTime(entry.at);
        // 状态点：fired/dismissed = 默认 text-dim，done = 成功，snoozed = 稍后，missed = 错误
        const dot = document.createElement("span");
        dot.className = `timeline-dot ${entry.status}`;
        dot.title = historyStatusLabel(entry.status);
        const title = document.createElement("p");
        title.className = "timeline-title";
        title.textContent = entry.title;
        row.append(time, dot, title);
        timeline.append(row);
      }
      pane.append(timeline);
    })
    .catch((err: unknown) => {
      pane.replaceChildren();
      const failed = document.createElement("p");
      failed.className = "field-error";
      failed.textContent = `加载失败：${formatError(err)}`;
      pane.append(failed);
    });
  return null;
}

/** 设置 Tab：分组（通用 / 声音 / 隐私与数据）+ 行式开关 + 数据目录。 */
function renderSettingsTab(pane: HTMLElement): null {
  pane.replaceChildren();
  const loading = document.createElement("p");
  loading.className = "hint";
  loading.textContent = "加载中…";
  pane.append(loading);

  void Promise.all([fetchSettings(), fetchDataDir()])
    .then(([settings, dir]) => {
      pane.replaceChildren();

      /** 保存整份设置（开关切换统一入口）。 */
      const persist = (next: AppSettingsView): void => {
        saveAppSettings(next)
          .then(() => {
            settings = next;
          })
          .catch((err: unknown) => {
            void alertDialog(formatError(err)).then(() => renderSettingsTab(pane));
          });
      };

      /** 一行开关设置。 */
      const settingSwitch = (
        label: string,
        checked: boolean,
        onChange: (value: boolean) => void,
        hint?: string,
      ): HTMLElement => {
        const row = document.createElement("div");
        row.className = "setting-row";
        const caption = document.createElement("div");
        caption.className = "setting-label";
        const name = document.createElement("div");
        name.textContent = label;
        caption.append(name);
        if (hint !== undefined) {
          const note = document.createElement("div");
          note.className = "hint";
          note.textContent = hint;
          caption.append(note);
        }
        const toggleLabel = document.createElement("label");
        toggleLabel.className = "switch";
        const input = document.createElement("input");
        input.type = "checkbox";
        input.checked = checked;
        input.addEventListener("change", () => onChange(input.checked));
        toggleLabel.append(input, document.createElement("span"));
        row.append(caption, toggleLabel);
        return row;
      };

      /** 分组容器：分组标题（11/600 text-dim + 字距）+ 行。 */
      const group = (title: string, rows: HTMLElement[]): HTMLElement => {
        const box = document.createElement("div");
        box.className = "settings-group";
        const head = document.createElement("p");
        head.className = "group-title";
        head.textContent = title;
        box.append(head, ...rows);
        return box;
      };

      // —— 通知权限：被拒后可在设置页重试请求，恢复调度器的系统通知分发
      const permRow = document.createElement("div");
      permRow.className = "setting-row";
      const permCaption = document.createElement("div");
      permCaption.className = "setting-label";
      const permName = document.createElement("div");
      permName.textContent = "通知权限";
      const permHint = document.createElement("div");
      permHint.className = "hint";
      permHint.textContent = "被拒后可在此重新请求";
      permCaption.append(permName, permHint);
      const permButton = document.createElement("button");
      permButton.type = "button";
      permButton.className = "button";
      permButton.textContent = "请求通知权限";
      permButton.addEventListener("click", () => {
        permButton.disabled = true;
        permHint.textContent = "请求中…";
        requestNotificationPermission()
          .then((state) => {
            permHint.textContent =
              state === "granted"
                ? "已获得通知权限"
                : "仍被拒绝，请在「系统设置 → 通知」中允许拾律后重试";
          })
          .catch((err: unknown) => {
            permHint.textContent = `请求失败：${formatError(err)}`;
          })
          .finally(() => {
            permButton.disabled = false;
          });
      });
      permRow.append(permCaption, permButton);

      // —— 数据目录：只读展示 + 访达打开
      const dirRow = document.createElement("div");
      dirRow.className = "setting-row";
      const dirCaption = document.createElement("div");
      dirCaption.className = "setting-label";
      const dirName = document.createElement("div");
      dirName.textContent = "数据目录";
      const dirValue = document.createElement("div");
      dirValue.className = "hint";
      dirValue.textContent = dir;
      dirCaption.append(dirName, dirValue);
      const openButton = document.createElement("button");
      openButton.type = "button";
      openButton.className = "button";
      openButton.textContent = "打开数据目录";
      openButton.addEventListener("click", () => {
        openDataDir().catch((err: unknown) => {
          void alertDialog(formatError(err));
        });
      });
      dirRow.append(dirCaption, openButton);

      pane.append(
        group("通用", [
          settingSwitch(
            "开机自启",
            settings.launch_at_login,
            (value) => persist({ ...settings, launch_at_login: value }),
            "请确保 App 在 /Applications（登录项按安装路径启动）",
          ),
          settingSwitch("仪表盘失焦隐藏", settings.dashboard_hide_on_blur, (value) =>
            persist({ ...settings, dashboard_hide_on_blur: value }),
          ),
        ]),
        group("声音", [
          settingSwitch("默认声音", settings.default_sound, (value) =>
            persist({ ...settings, default_sound: value }),
          ),
        ]),
        group("隐私与数据", [permRow, dirRow]),
      );
    })
    .catch((err: unknown) => {
      pane.replaceChildren();
      const failed = document.createElement("p");
      failed.className = "field-error";
      failed.textContent = `加载设置失败：${formatError(err)}`;
      pane.append(failed);
    });
  return null;
}

/** 构建主窗口视图。 */
export function mainView(): HTMLElement {
  const root = document.createElement("section");
  root.className = "view main";

  // 顶部栏：Tab 列表（左）+ 右上 ghost「+ 新增」（仅提醒页可见）
  const topBar = document.createElement("div");
  topBar.className = "main-topbar";
  const tabBar = document.createElement("nav");
  tabBar.className = "tab-bar";
  const spacer = document.createElement("div");
  spacer.className = "main-topbar-spacer";
  const addButton = document.createElement("button");
  addButton.type = "button";
  addButton.className = "button ghost";
  addButton.textContent = "+ 新增";
  addButton.hidden = true;
  topBar.append(tabBar, spacer, addButton);
  const content = document.createElement("div");
  content.className = "tab-content";

  const specs: TabSpec[] = [
    { id: "reminders", title: "提醒", refresh: renderRemindersTab },
    { id: "history", title: "历史", refresh: renderHistoryTab },
    { id: "settings", title: "设置", refresh: renderSettingsTab },
  ];
  const handles: TabHandle[] = specs.map((spec) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "tab-button";
    button.textContent = spec.title;
    const pane = document.createElement("div");
    pane.className = "tab-pane";
    tabBar.append(button);
    content.append(pane);
    return { spec, button, pane };
  });

  /** 提醒 Tab 句柄（供「+ 新增」触发；表单挂载中为空操作）。 */
  let remindersHandle: RemindersTabHandle | null = null;

  /** 切换到指定 Tab 并重建其内容。 */
  const activate = (id: string): void => {
    for (const handle of handles) {
      const active = handle.spec.id === id;
      handle.pane.hidden = !active;
      handle.button.classList.toggle("active", active);
      if (active) {
        remindersHandle = handle.spec.refresh(handle.pane);
      }
    }
    addButton.hidden = id !== "reminders";
  };

  addButton.addEventListener("click", () => {
    remindersHandle?.openNew();
  });

  for (const handle of handles) {
    handle.button.addEventListener("click", () => activate(handle.spec.id));
  }

  // 外部修改提醒数据（CLI 等）→ 重载提醒与历史两个数据 Tab
  onStoreChanged(() => {
    for (const handle of handles) {
      if (!handle.pane.hidden && handle.spec.id !== "settings") {
        remindersHandle = handle.spec.refresh(handle.pane) ?? remindersHandle;
      }
    }
  });

  root.append(topBar, content);
  activate(specs[0]!.id);
  return root;
}
