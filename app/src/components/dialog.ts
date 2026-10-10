/**
 * 页内模态对话框（WKWebView 的 window.confirm/alert 行为不可靠，统一走自绘）。
 * 用 Promise.withResolvers 暴露「结果 promise + 外部 settle」的线性控制流。
 */

/** 异步确认框；返回用户是否确认（默认焦点在取消，防误触）。 */
export function confirmDialog(message: string, confirmLabel = "删除"): Promise<boolean> {
  const { promise, resolve } = Promise.withResolvers<boolean>();
  const overlay = document.createElement("div");
  overlay.className = "dialog-overlay";
  const box = document.createElement("div");
  box.className = "dialog";
  const text = document.createElement("p");
  text.className = "dialog-message";
  text.textContent = message;
  const actions = document.createElement("div");
  actions.className = "dialog-actions";
  const cancel = document.createElement("button");
  cancel.type = "button";
  cancel.className = "button";
  cancel.textContent = "取消";
  const confirm = document.createElement("button");
  confirm.type = "button";
  confirm.className = "button danger";
  confirm.textContent = confirmLabel;

  const settle = (value: boolean): void => {
    document.removeEventListener("keydown", onKeydown);
    overlay.remove();
    resolve(value);
  };
  const onKeydown = (event: KeyboardEvent): void => {
    if (event.key === "Escape") {
      settle(false);
    }
  };
  cancel.addEventListener("click", () => settle(false));
  confirm.addEventListener("click", () => settle(true));
  document.addEventListener("keydown", onKeydown);

  actions.append(cancel, confirm);
  box.append(text, actions);
  overlay.append(box);
  document.body.append(overlay);
  cancel.focus();
  return promise;
}

/** 异步提示框（仅「确定」）。 */
export function alertDialog(message: string): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  const overlay = document.createElement("div");
  overlay.className = "dialog-overlay";
  const box = document.createElement("div");
  box.className = "dialog";
  const text = document.createElement("p");
  text.className = "dialog-message";
  text.textContent = message;
  const actions = document.createElement("div");
  actions.className = "dialog-actions";
  const ok = document.createElement("button");
  ok.type = "button";
  ok.className = "button primary";
  ok.textContent = "确定";

  const settle = (): void => {
    document.removeEventListener("keydown", onKeydown);
    overlay.remove();
    resolve();
  };
  const onKeydown = (event: KeyboardEvent): void => {
    if (event.key === "Escape" || event.key === "Enter") {
      settle();
    }
  };
  ok.addEventListener("click", settle);
  document.addEventListener("keydown", onKeydown);

  actions.append(ok);
  box.append(text, actions);
  overlay.append(box);
  document.body.append(overlay);
  ok.focus();
  return promise;
}
