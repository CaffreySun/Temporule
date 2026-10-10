/**
 * 前端入口：按窗口 label 分发视图。
 * dashboard → 仪表盘；main → 主窗口（提醒/历史/设置 Tab）；popup-* → 弹窗。
 */
import { dashboardView } from "./views/dashboard-view";
import { mainView } from "./views/main-view";
import { popupView } from "./views/popup-view";

const root = document.getElementById("app");
if (root === null) {
  throw new Error("缺少 #app 挂载点");
}

const label: string = window.__TAURI__.window.getCurrentWindow().label;

if (label.startsWith("popup-")) {
  root.append(popupView());
} else {
  switch (label) {
    case "main":
      root.append(mainView());
      break;
    case "dashboard":
    default:
      root.append(dashboardView());
      break;
  }
}
