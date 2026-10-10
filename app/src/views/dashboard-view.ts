/**
 * 仪表盘占位视图（切片 4 实装提醒列表与表单）。
 * 组件约定：组件 = 一个返回 HTMLElement 的函数。
 */

/** 构建仪表盘占位视图。 */
export function dashboardView(): HTMLElement {
  const root = document.createElement("section");
  root.className = "view dashboard";

  const header = document.createElement("header");
  header.className = "view-header";

  const title = document.createElement("h1");
  title.textContent = "拾律";

  const status = document.createElement("span");
  status.className = "status-dot idle";
  status.title = "调度器未接入（切片 5 实装）";

  header.append(title, status);

  const hint = document.createElement("p");
  hint.className = "hint";
  hint.textContent = "仪表盘占位视图：提醒列表 / 新增 / 打开主窗口（切片 4 实装）";

  root.append(header, hint);
  return root;
}
