/**
 * 弹窗视图（label 前缀 `popup-`）：标题、备注、「完成 / 稍后」按钮、AutoDismiss 倒计时展示。
 *
 * URL 形如 `index.html?view=popup&pid={reminder_id}#{popup_id}`（每窗唯一，避开
 * WKWebView 页面缓存串扰）：hash 直接携带 popup_id；同 reminder 二次触发时 Rust 复用窗口
 * 并发 `popup-replace` 事件注入新内容（倒计时展示随之重置）。AutoDismiss 的实际关窗与
 * history(dismissed) 由 Rust 计时统一裁决，前端倒计时只做展示；提醒已删（popup_detail
 * 返回空）时视图自行关窗。
 */
import { fetchPopupDetail, formatError, popupAction, type PopupDetailView } from "../api";

/** 弹窗替换事件名（与 Rust 的 popup::POPUP_REPLACE_EVENT 对齐）。 */
const POPUP_REPLACE_EVENT = "popup-replace";

/** 构建弹窗视图。 */
export function popupView(): HTMLElement {
  const root = document.createElement("section");
  root.className = "view popup";

  // URL hash 直接携带 popup_id（#{popup_id}），无 hash 视为非法直接关窗
  let popupId: string | null = window.location.hash.slice(1) || null;
  /** AutoDismiss 倒计时展示句柄（纯展示，不负责关窗）。 */
  let countdown: number | null = null;

  const title = document.createElement("h2");
  title.className = "popup-title";
  // 无边框弹窗自绘标题栏：标题整条可拖动，右侧关闭按钮（关闭同样汇聚 Destroyed 裁决）
  const titleBar = document.createElement("div");
  titleBar.className = "popup-header title-bar";
  titleBar.setAttribute("data-tauri-drag-region", "deep");
  const closeButton = document.createElement("button");
  closeButton.type = "button";
  closeButton.className = "button small window-close";
  closeButton.textContent = "✕";
  closeButton.title = "关闭弹窗";
  titleBar.append(title, closeButton);
  const note = document.createElement("p");
  note.className = "popup-note muted";
  const countdownLabel = document.createElement("span");
  countdownLabel.className = "popup-countdown muted";
  const doneButton = document.createElement("button");
  doneButton.type = "button";
  doneButton.className = "button primary";
  doneButton.textContent = "完成";
  const snoozeButton = document.createElement("button");
  snoozeButton.type = "button";
  snoozeButton.className = "button";
  snoozeButton.textContent = "稍后";
  const actions = document.createElement("div");
  actions.className = "popup-actions";
  actions.append(doneButton, snoozeButton, countdownLabel);
  root.append(titleBar, note, actions);

  /** 停止倒计时展示并关窗（Rust 端 popup_action / AutoDismiss 也会关窗，此处兜底收尾）。 */
  const close = (): void => {
    if (countdown !== null) {
      window.clearInterval(countdown);
      countdown = null;
    }
    void window.__TAURI__.window
      .getCurrentWindow()
      .close()
      .catch(() => undefined);
  };

  /** 按钮动作：调 popup_action（done|snooze），Rust 记 history 并关窗。 */
  const act = (action: "done" | "snooze"): void => {
    if (popupId === null) {
      close();
      return;
    }
    if (countdown !== null) {
      window.clearInterval(countdown);
      countdown = null;
    }
    popupAction(popupId, action)
      .catch(() => undefined)
      .finally(close);
  };
  doneButton.addEventListener("click", () => act("done"));
  snoozeButton.addEventListener("click", () => act("snooze"));
  closeButton.addEventListener("click", () => close());

  /** 渲染弹窗内容；AutoDismiss 显示秒数倒计时，RequireAction 不显示。 */
  const render = (detail: PopupDetailView): void => {
    title.textContent = detail.title;
    note.textContent = detail.note ?? "";
    note.hidden = detail.note === null;
    if (countdown !== null) {
      window.clearInterval(countdown);
      countdown = null;
    }
    if (detail.alert.kind === "force") {
      countdownLabel.textContent = "";
      return;
    }
    let left = detail.alert.seconds;
    countdownLabel.textContent = `${left} 秒后自动关闭`;
    countdown = window.setInterval(() => {
      left -= 1;
      countdownLabel.textContent = left > 0 ? `${left} 秒后自动关闭` : "即将自动关闭…";
      if (left <= 0 && countdown !== null) {
        window.clearInterval(countdown);
        countdown = null;
      }
    }, 1000);
  };

  /** 拉取并渲染 popup_id 对应内容；提醒已删（返回空）→ 自行关窗，拉取失败不关窗。 */
  const load = (id: string): void => {
    fetchPopupDetail(id)
      .then((detail) => {
        if (detail === null) {
          close();
          return;
        }
        render(detail);
      })
      .catch((err: unknown) => {
        // 不关窗：避免误吞一个未处理的弹窗
        title.textContent = "弹窗内容加载失败";
        note.textContent = formatError(err);
      });
  };

  if (popupId === null) {
    close();
  } else {
    load(popupId);
  }

  // 同 reminder 二次触发：Rust 复用本窗口注入新内容，popup_id 随之更新
  void window.__TAURI__.event
    .listen<{ popup_id: string }>(POPUP_REPLACE_EVENT, (event) => {
      popupId = event.payload.popup_id;
      window.location.hash = `#/popup/${popupId}`;
      load(popupId);
    })
    .catch(() => undefined);

  return root;
}
