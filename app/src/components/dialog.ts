/**
 * 页内模态对话框（WKWebView 的 window.confirm/alert 行为不可靠，统一走自绘）。
 * 用 Promise.withResolvers 暴露「结果 promise + 外部 settle」的线性控制流。
 */
import { openExternal } from "../api";
import logoUrl from "../assets/Temporule_AppIcon.png";

/** GitHub 仓库地址（关于面板链接）。 */
const GITHUB_REPO_URL = "https://github.com/CaffreySun/Temporule";

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

/**
 * 「关于拾律」面板：logo + 应用名 + 版本 + 许可信息 + GitHub 链接。
 * 沿用现有 dialog 交互约定：✕ / Esc 关闭，点击遮罩不关闭。
 */
export function aboutDialog(name: string, version: string): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  const overlay = document.createElement("div");
  overlay.className = "dialog-overlay";
  const box = document.createElement("div");
  box.className = "dialog about";

  const logo = document.createElement("img");
  logo.className = "about-logo";
  logo.src = logoUrl;
  logo.alt = `${name} 图标`;

  const title = document.createElement("p");
  title.className = "about-name";
  title.textContent = name;

  const versionLine = document.createElement("p");
  versionLine.className = "about-version";
  versionLine.textContent = `版本 ${version}`;

  const license = document.createElement("p");
  license.className = "about-license";
  license.textContent = "MIT License · 开源项目";

  const link = document.createElement("button");
  link.type = "button";
  link.className = "about-link";
  link.textContent = "GitHub 仓库";

  const close = document.createElement("button");
  close.type = "button";
  close.className = "about-close";
  close.textContent = "✕";
  close.setAttribute("aria-label", "关闭关于面板");

  const settle = (): void => {
    document.removeEventListener("keydown", onKeydown);
    overlay.remove();
    resolve();
  };
  const onKeydown = (event: KeyboardEvent): void => {
    if (event.key === "Escape") {
      settle();
    }
  };
  link.addEventListener("click", () => {
    openExternal(GITHUB_REPO_URL).catch(() => {
      // 打开失败不阻塞面板，用户可自行复制仓库地址
    });
  });
  close.addEventListener("click", settle);
  document.addEventListener("keydown", onKeydown);

  box.append(close, logo, title, versionLine, license, link);
  overlay.append(box);
  document.body.append(overlay);
  close.focus();
  return promise;
}
