/**
 * 仪表盘视图：标题「拾律」+ 运行状态圆点、提醒卡片列表（排程摘要/下次触发/启用 switch/
 * 编辑/删除带确认）、「+ 新增」切表单视图、底部「打开主窗口」。
 * 组件约定：组件 = 一个返回 HTMLElement 的函数。
 */
import {
  formatDateTime,
  formatError,
  getReminder,
  isSaveConflictError,
  listReminders,
  onStoreChanged,
  removeReminder,
  showMainWindow,
  toggleReminder,
  type ReminderDetail,
  type ReminderView,
} from "../api";
import { alertDialog, confirmDialog } from "../components/dialog";
import { reminderForm } from "../components/reminder-form";

/** 一张提醒卡片（标题/排程摘要/下次触发/启用 switch/编辑/删除）。 */
function reminderCard(
  reminder: ReminderView,
  onEdit: (detail: ReminderDetail) => void,
  onChanged: () => void,
): HTMLElement {
  const card = document.createElement("article");
  card.className = "card";
  const head = document.createElement("div");
  head.className = "card-head";
  const title = document.createElement("h3");
  title.textContent = reminder.title;
  const toggleLabel = document.createElement("label");
  toggleLabel.className = "switch";
  toggleLabel.title = reminder.enabled ? "已启用，点击停用" : "已停用，点击启用";
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
  head.append(title, toggleLabel);

  const desc = document.createElement("p");
  desc.className = "muted";
  desc.textContent = reminder.schedule_desc;
  const next = document.createElement("p");
  next.className = "card-next";
  next.textContent = `下次触发：${formatDateTime(reminder.next_fire)}`;

  const actions = document.createElement("div");
  actions.className = "card-actions";
  const editButton = document.createElement("button");
  editButton.type = "button";
  editButton.className = "button";
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
  removeButton.className = "button danger";
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
  actions.append(editButton, removeButton);
  card.append(head, desc, next, actions);
  return card;
}

/** 构建仪表盘视图。 */
export function dashboardView(): HTMLElement {
  const root = document.createElement("section");
  root.className = "view dashboard";

  const header = document.createElement("header");
  header.className = "view-header";
  const title = document.createElement("h1");
  title.textContent = "拾律";
  const status = document.createElement("span");
  status.className = "status-dot running";
  status.title = "调度器运行中";
  header.append(title, status);

  const content = document.createElement("div");
  content.className = "dashboard-content";

  const footer = document.createElement("footer");
  footer.className = "view-footer";
  const openMain = document.createElement("button");
  openMain.type = "button";
  openMain.className = "button primary wide";
  openMain.textContent = "打开主窗口";
  openMain.addEventListener("click", () => {
    showMainWindow().catch((err: unknown) => {
      void alertDialog(formatError(err));
    });
  });
  footer.append(openMain);

  /** 切回列表视图并重载（新增/编辑/冲突后的统一收尾）。 */
  const showList = (): void => {
    content.replaceChildren();
    const toolbar = document.createElement("div");
    toolbar.className = "toolbar";
    const addButton = document.createElement("button");
    addButton.type = "button";
    addButton.className = "button primary";
    addButton.textContent = "+ 新增";
    addButton.addEventListener("click", () => showForm(undefined));
    toolbar.append(addButton);
    const listBox = document.createElement("div");
    listBox.className = "card-list";
    content.append(toolbar, listBox);
    void renderCards(listBox);
  };

  /** 切到表单视图（新增或编辑）。 */
  const showForm = (initial: ReminderDetail | undefined): void => {
    content.replaceChildren(
      reminderForm({
        initial,
        onFinished: showList,
        onCancel: showList,
      }),
    );
  };

  /** 拉取并渲染卡片列表。 */
  const renderCards = (container: HTMLElement): void => {
    container.textContent = "加载中…";
    listReminders()
      .then((reminders) => {
        container.replaceChildren();
        if (reminders.length === 0) {
          const empty = document.createElement("p");
          empty.className = "hint";
          empty.textContent = "暂无提醒，点击「+ 新增」创建第一条。";
          container.append(empty);
          return;
        }
        for (const reminder of reminders) {
          container.append(reminderCard(reminder, (detail) => showForm(detail), showList));
        }
      })
      .catch((err: unknown) => {
        container.replaceChildren();
        const failed = document.createElement("p");
        failed.className = "field-error";
        failed.textContent = `加载失败：${formatError(err)}`;
        container.append(failed);
      });
  };

  // 外部修改提醒数据（CLI 等）→ 自动重载列表；表单挂载期间挂起重载，
  // 保存/取消收尾时本就重载一次，不吞未保存输入
  onStoreChanged(() => {
    if (content.querySelector("form.reminder-form") === null) {
      showList();
    }
  });

  root.append(header, content, footer);
  showList();
  return root;
}
