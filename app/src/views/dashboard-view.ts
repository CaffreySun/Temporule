/**
 * 仪表盘视图（v2）：自绘标题区（拾律 + 状态点 + ✕，可拖动）→ 行列表（v2 Row）→
 * 底部 toolbar（[+ 新增] / [主窗口 ↗]，ghost 按钮）。表单挂载期间底部「+ 新增」挂起
 * （不吞未保存输入），store-changed 同样挂起重载。
 * 组件约定：组件 = 一个返回 HTMLElement 的函数。
 */
import {
  formatError,
  listReminders,
  onStoreChanged,
  showMainWindow,
  type ReminderDetail,
} from "../api";
import { alertDialog } from "../components/dialog";
import { reminderForm } from "../components/reminder-form";
import { reminderRow } from "../components/reminder-row";

/** 构建仪表盘视图。 */
export function dashboardView(): HTMLElement {
  const root = document.createElement("section");
  root.className = "view dashboard";

  const header = document.createElement("header");
  header.className = "view-header title-bar";
  header.setAttribute("data-tauri-drag-region", "deep");
  const title = document.createElement("h1");
  title.textContent = "拾律";
  const status = document.createElement("span");
  status.className = "status-dot running";
  status.title = "调度器运行中";
  // 无边框窗口自绘关闭按钮（等价原生红灯：关窗并按剩余可见窗口回切激活策略）
  const closeButton = document.createElement("button");
  closeButton.type = "button";
  closeButton.className = "button small window-close";
  closeButton.textContent = "✕";
  closeButton.title = "关闭仪表盘";
  closeButton.addEventListener("click", () => {
    void window.__TAURI__.window
      .getCurrentWindow()
      .close()
      .catch((err: unknown) => {
        void alertDialog(formatError(err));
      });
  });
  header.append(title, status, closeButton);

  const content = document.createElement("div");
  content.className = "dashboard-content";

  // 底部 toolbar：ghost 按钮（透明底、hover surface-raised）
  const footer = document.createElement("footer");
  footer.className = "toolbar footer";
  const addButton = document.createElement("button");
  addButton.type = "button";
  addButton.className = "button ghost";
  addButton.textContent = "+ 新增";
  addButton.addEventListener("click", () => {
    // 表单挂载中挂起：不吞未保存输入
    if (content.querySelector("form.reminder-form") === null) {
      showForm(undefined);
    }
  });
  const spacer = document.createElement("div");
  spacer.className = "spacer";
  const openMain = document.createElement("button");
  openMain.type = "button";
  openMain.className = "button ghost";
  openMain.textContent = "主窗口 ↗";
  openMain.addEventListener("click", () => {
    showMainWindow().catch((err: unknown) => {
      void alertDialog(formatError(err));
    });
  });
  footer.append(addButton, spacer, openMain);

  /** 切回列表视图并重载（新增/编辑/冲突后的统一收尾）。 */
  const showList = (): void => {
    content.replaceChildren();
    const listBox = document.createElement("div");
    listBox.className = "row-list";
    const loading = document.createElement("p");
    loading.className = "hint";
    loading.textContent = "加载中…";
    listBox.append(loading);
    content.append(listBox);
    void renderRows(listBox);
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

  /** 拉取并渲染行列表。 */
  const renderRows = (container: HTMLElement): void => {
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
          container.append(
            reminderRow({
              reminder,
              onEdit: (detail) => showForm(detail),
              onChanged: showList,
            }),
          );
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
