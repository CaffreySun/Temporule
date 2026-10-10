/**
 * 主窗口占位视图：提醒 / 历史 / 设置 三个 Tab（切片 4 实装内容）。
 * 组件约定：组件 = 一个返回 HTMLElement 的函数。
 */

/** Tab 标题配置。 */
interface TabSpec {
  /** Tab 标识。 */
  id: string;
  /** Tab 中文标题。 */
  title: string;
  /** 占位说明文案。 */
  placeholder: string;
}

/** 一个 Tab 的完整句柄。 */
interface TabHandle {
  spec: TabSpec;
  button: HTMLButtonElement;
  pane: HTMLElement;
}

const TABS: readonly TabSpec[] = [
  { id: "reminders", title: "提醒", placeholder: "提醒列表占位（切片 4 实装）" },
  { id: "history", title: "历史", placeholder: "历史记录占位（切片 4 实装）" },
  { id: "settings", title: "设置", placeholder: "设置占位（切片 4 实装）" },
];

/** 构建主窗口占位视图。 */
export function mainView(): HTMLElement {
  const root = document.createElement("section");
  root.className = "view main";

  const tabBar = document.createElement("nav");
  tabBar.className = "tab-bar";

  const content = document.createElement("div");
  content.className = "tab-content";

  const handles: TabHandle[] = TABS.map((spec) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "tab-button";
    button.textContent = spec.title;

    const pane = document.createElement("div");
    pane.className = "tab-pane";
    const hint = document.createElement("p");
    hint.className = "hint";
    hint.textContent = spec.placeholder;
    pane.append(hint);

    tabBar.append(button);
    content.append(pane);
    return { spec, button, pane };
  });

  /** 切换到指定 Tab。 */
  function activate(id: string): void {
    for (const handle of handles) {
      const active = handle.spec.id === id;
      handle.pane.hidden = !active;
      handle.button.classList.toggle("active", active);
    }
  }

  for (const handle of handles) {
    handle.button.addEventListener("click", () => activate(handle.spec.id));
  }

  root.append(tabBar, content);
  activate(TABS[0]!.id);
  return root;
}
