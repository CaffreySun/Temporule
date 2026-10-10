/**
 * 主窗口视图：提醒（完整表格 + 新增/编辑/删除）/ 历史（表格，状态中文）/ 设置
 * （三个开关 + 数据目录展示 + 打开数据目录）。
 * 组件约定：组件 = 一个返回 HTMLElement 的函数。
 */
import {
  fetchHistory,
  fetchDataDir,
  fetchSettings,
  formatDateTime,
  formatError,
  getReminder,
  historyStatusLabel,
  isSaveConflictError,
  listReminders,
  onStoreChanged,
  openDataDir,
  removeReminder,
  saveAppSettings,
  toggleReminder,
  type AppSettingsView,
  type ReminderDetail,
  type ReminderView,
} from "../api";
import { alertDialog, confirmDialog } from "../components/dialog";
import { reminderForm } from "../components/reminder-form";

/** 历史表格展示上限。 */
const HISTORY_LIMIT = 200;

/** Tab 配置：标题 + 激活时的重建函数（由 mainView 注入）。 */
interface TabSpec {
  /** Tab 标识。 */
  id: string;
  /** Tab 中文标题。 */
  title: string;
  /** 激活该 Tab 时重建内容。 */
  refresh: (pane: HTMLElement) => void;
}

/** 一个 Tab 的完整句柄。 */
interface TabHandle {
  spec: TabSpec;
  button: HTMLButtonElement;
  pane: HTMLElement;
}

/** 提醒表格行。 */
function reminderRow(
  reminder: ReminderView,
  onEdit: (detail: ReminderDetail) => void,
  onChanged: () => void,
): HTMLTableRowElement {
  const row = document.createElement("tr");
  const titleCell = document.createElement("td");
  titleCell.textContent = reminder.title;
  if (reminder.note !== null && reminder.note !== "") {
    titleCell.title = reminder.note;
  }
  const descCell = document.createElement("td");
  descCell.className = "muted";
  descCell.textContent = reminder.schedule_desc;
  const enabledCell = document.createElement("td");
  const toggleLabel = document.createElement("label");
  toggleLabel.className = "switch";
  const toggleInput = document.createElement("input");
  toggleInput.type = "checkbox";
  toggleInput.checked = reminder.enabled;
  toggleInput.addEventListener("change", () => {
    const enabled = toggleInput.checked;
    toggleReminder(reminder.id, enabled)
      .then(onChanged)
      .catch((err: unknown) => {
        toggleInput.checked = !enabled;
        if (isSaveConflictError(err)) {
          void alertDialog("数据已被外部修改，请重试").then(onChanged);
        } else {
          void alertDialog(formatError(err));
        }
      });
  });
  toggleLabel.append(toggleInput, document.createElement("span"));
  enabledCell.append(toggleLabel);
  const nextCell = document.createElement("td");
  nextCell.textContent = formatDateTime(reminder.next_fire);
  const actionsCell = document.createElement("td");
  const editButton = document.createElement("button");
  editButton.type = "button";
  editButton.className = "button small";
  editButton.textContent = "编辑";
  editButton.addEventListener("click", () => {
    getReminder(reminder.id)
      .then(onEdit)
      .catch((err: unknown) => {
        void alertDialog(formatError(err));
      });
  });
  const removeButton = document.createElement("button");
  removeButton.type = "button";
  removeButton.className = "button small danger";
  removeButton.textContent = "删除";
  removeButton.addEventListener("click", () => {
    void confirmDialog(`确认删除「${reminder.title}」？`).then((confirmed) => {
      if (confirmed) {
        removeReminder(reminder.id)
          .then(onChanged)
          .catch((err: unknown) => {
            void alertDialog(formatError(err));
          });
      }
    });
  });
  actionsCell.append(editButton, removeButton);
  row.append(titleCell, descCell, enabledCell, nextCell, actionsCell);
  return row;
}

/** 提醒 Tab：表格 ↔ 表单 双态。 */
function renderRemindersTab(pane: HTMLElement): void {
  const showTable = (): void => {
    pane.replaceChildren();
    const toolbar = document.createElement("div");
    toolbar.className = "toolbar";
    const addButton = document.createElement("button");
    addButton.type = "button";
    addButton.className = "button primary";
    addButton.textContent = "+ 新增";
    addButton.addEventListener("click", () => showForm(undefined));
    toolbar.append(addButton);
    const table = document.createElement("table");
    table.className = "data-table";
    const head = document.createElement("thead");
    const headRow = document.createElement("tr");
    for (const label of ["标题", "排程摘要", "启用", "下次触发", "操作"]) {
      const th = document.createElement("th");
      th.textContent = label;
      headRow.append(th);
    }
    head.append(headRow);
    const body = document.createElement("tbody");
    table.append(head, body);
    pane.append(toolbar, table);
    body.textContent = "";
    const loading = document.createElement("tr");
    const loadingCell = document.createElement("td");
    loadingCell.colSpan = 5;
    loadingCell.className = "hint";
    loadingCell.textContent = "加载中…";
    loading.append(loadingCell);
    body.append(loading);

    listReminders()
      .then((reminders) => {
        body.replaceChildren();
        if (reminders.length === 0) {
          const empty = document.createElement("tr");
          const emptyCell = document.createElement("td");
          emptyCell.colSpan = 5;
          emptyCell.className = "hint";
          emptyCell.textContent = "暂无提醒，点击「+ 新增」创建第一条。";
          empty.append(emptyCell);
          body.append(empty);
          return;
        }
        for (const reminder of reminders) {
          body.append(reminderRow(reminder, (detail) => showForm(detail), showTable));
        }
      })
      .catch((err: unknown) => {
        body.replaceChildren();
        const failed = document.createElement("tr");
        const failedCell = document.createElement("td");
        failedCell.colSpan = 5;
        failedCell.className = "field-error";
        failedCell.textContent = `加载失败：${formatError(err)}`;
        failed.append(failedCell);
        body.append(failed);
      });
  };

  const showForm = (initial: ReminderDetail | undefined): void => {
    pane.replaceChildren(
      reminderForm({
        initial,
        onFinished: showTable,
        onCancel: showTable,
      }),
    );
  };

  showTable();
}

/** 历史 Tab：时间 / 标题 / 状态 / 计划触发，新→旧。 */
function renderHistoryTab(pane: HTMLElement): void {
  pane.replaceChildren();
  const table = document.createElement("table");
  table.className = "data-table";
  const head = document.createElement("thead");
  const headRow = document.createElement("tr");
  for (const label of ["时间", "标题", "状态", "计划触发"]) {
    const th = document.createElement("th");
    th.textContent = label;
    headRow.append(th);
  }
  head.append(headRow);
  const body = document.createElement("tbody");
  table.append(head, body);
  pane.append(table);
  const loading = document.createElement("tr");
  const loadingCell = document.createElement("td");
  loadingCell.colSpan = 4;
  loadingCell.className = "hint";
  loadingCell.textContent = "加载中…";
  loading.append(loadingCell);
  body.append(loading);

  fetchHistory(HISTORY_LIMIT)
    .then((entries) => {
      body.replaceChildren();
      if (entries.length === 0) {
        const empty = document.createElement("tr");
        const emptyCell = document.createElement("td");
        emptyCell.colSpan = 4;
        emptyCell.className = "hint";
        emptyCell.textContent = "暂无历史记录。";
        empty.append(emptyCell);
        body.append(empty);
        return;
      }
      for (const entry of [...entries].reverse()) {
        const row = document.createElement("tr");
        const atCell = document.createElement("td");
        atCell.textContent = formatDateTime(entry.at);
        const titleCell = document.createElement("td");
        titleCell.textContent = entry.title;
        const statusCell = document.createElement("td");
        statusCell.textContent = historyStatusLabel(entry.status);
        const scheduledCell = document.createElement("td");
        scheduledCell.textContent = formatDateTime(entry.scheduled_at);
        row.append(atCell, titleCell, statusCell, scheduledCell);
        body.append(row);
      }
    })
    .catch((err: unknown) => {
      body.replaceChildren();
      const failed = document.createElement("tr");
      const failedCell = document.createElement("td");
      failedCell.colSpan = 4;
      failedCell.className = "field-error";
      failedCell.textContent = `加载失败：${formatError(err)}`;
      failed.append(failedCell);
      body.append(failed);
    });
}

/** 设置 Tab：三个开关 + 数据目录只读展示 + 打开数据目录。 */
function renderSettingsTab(pane: HTMLElement): void {
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

      pane.append(
        settingSwitch(
          "开机自启",
          settings.launch_at_login,
          (value) => persist({ ...settings, launch_at_login: value }),
          "请确保 App 在 /Applications（登录项按安装路径启动）",
        ),
        settingSwitch("默认声音", settings.default_sound, (value) =>
          persist({ ...settings, default_sound: value }),
        ),
        settingSwitch("仪表盘失焦隐藏", settings.dashboard_hide_on_blur, (value) =>
          persist({ ...settings, dashboard_hide_on_blur: value }),
        ),
      );

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
      pane.append(dirRow);
    })
    .catch((err: unknown) => {
      pane.replaceChildren();
      const failed = document.createElement("p");
      failed.className = "field-error";
      failed.textContent = `加载设置失败：${formatError(err)}`;
      pane.append(failed);
    });
}

/** 构建主窗口视图。 */
export function mainView(): HTMLElement {
  const root = document.createElement("section");
  root.className = "view main";

  const tabBar = document.createElement("nav");
  tabBar.className = "tab-bar";
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

  /** 切换到指定 Tab 并重建其内容。 */
  const activate = (id: string): void => {
    for (const handle of handles) {
      const active = handle.spec.id === id;
      handle.pane.hidden = !active;
      handle.button.classList.toggle("active", active);
      if (active) {
        handle.spec.refresh(handle.pane);
      }
    }
  };

  for (const handle of handles) {
    handle.button.addEventListener("click", () => activate(handle.spec.id));
  }

  // 外部修改提醒数据（CLI 等）→ 重载提醒与历史两个数据 Tab
  onStoreChanged(() => {
    for (const handle of handles) {
      if (!handle.pane.hidden && handle.spec.id !== "settings") {
        handle.spec.refresh(handle.pane);
      }
    }
  });

  root.append(tabBar, content);
  activate(specs[0]!.id);
  return root;
}
