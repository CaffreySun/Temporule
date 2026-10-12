/**
 * 提醒行（v2 Row 组件，仪表盘与主窗口提醒页共用）：状态点 + 标题/摘要两行堆叠 +
 * 右侧 switch；hover 整行提亮、行尾淡入 编辑/删除 图标按钮（删除仍走 confirmDialog）。
 * 组件约定：组件 = 一个返回 HTMLElement 的函数。
 */
import {
  formatDateTime,
  formatError,
  getReminder,
  isSaveConflictError,
  removeReminder,
  toggleReminder,
  type ReminderDetail,
  type ReminderView,
} from "../api";
import { alertDialog, confirmDialog } from "./dialog";

/** 行组件选项。 */
export interface ReminderRowOptions {
  /** 行数据。 */
  reminder: ReminderView;
  /** 编辑回调（拉取完整字段后由调用方切到表单视图）。 */
  onEdit: (detail: ReminderDetail) => void;
  /** 数据变更后的收尾（调用方重载列表）。 */
  onChanged: () => void;
}

/** 16px 单色 SVG 图标（currentColor 着色）。 */
function icon(pathD: string): SVGSVGElement {
  const svgNs = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(svgNs, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", "16");
  svg.setAttribute("height", "16");
  svg.setAttribute("fill", "currentColor");
  const path = document.createElementNS(svgNs, "path");
  path.setAttribute("d", pathD);
  svg.append(path);
  return svg;
}

/** 编辑（铅笔）图标路径。 */
const ICON_EDIT =
  "M3 17.25V21h3.75L17.8 9.94l-3.75-3.75L3 17.25zM20.71 7.04a1 1 0 0 0 0-1.41l-2.34-2.34a1 1 0 0 0-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83z";

/** 删除（垃圾桶）图标路径。 */
const ICON_DELETE = "M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z";

/** 构建一条提醒行。 */
export function reminderRow(options: ReminderRowOptions): HTMLElement {
  const { reminder, onEdit, onChanged } = options;

  const row = document.createElement("div");
  row.className = "row";

  // 左：状态点（启用 = --success，停用 = --text-dim）
  const dot = document.createElement("span");
  dot.className = reminder.enabled ? "row-dot on" : "row-dot";
  dot.title = reminder.enabled ? "已启用" : "已停用";

  // 中：标题（13/500）+ 摘要（11/400 text-dim，一行省略）
  const main = document.createElement("div");
  main.className = "row-main";
  const title = document.createElement("p");
  title.className = "row-title";
  title.textContent = reminder.title;
  if (reminder.note !== null && reminder.note !== "") {
    title.title = reminder.note;
  }
  const subtitle = document.createElement("p");
  subtitle.className = "row-subtitle";
  subtitle.textContent = `下次触发 ${formatDateTime(reminder.next_fire)} · ${reminder.schedule_desc}`;
  main.append(title, subtitle);

  // hover 才淡入的行尾操作（switch 左侧）：编辑 / 删除
  const actions = document.createElement("div");
  actions.className = "row-actions";
  const editButton = document.createElement("button");
  editButton.type = "button";
  editButton.className = "icon-button";
  editButton.title = "编辑";
  editButton.append(icon(ICON_EDIT));
  editButton.addEventListener("click", () => {
    getReminder(reminder.id)
      .then(onEdit)
      .catch((err: unknown) => {
        void alertDialog(formatError(err));
      });
  });
  const removeButton = document.createElement("button");
  removeButton.type = "button";
  removeButton.className = "icon-button";
  removeButton.title = "删除";
  removeButton.append(icon(ICON_DELETE));
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

  // 右：启用 switch
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

  row.append(dot, main, actions, toggleLabel);
  return row;
}
