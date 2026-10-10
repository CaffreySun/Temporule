/**
 * Tauri 全局注入 API 的最小类型声明（tauri.conf.json 中 app.withGlobalTauri = true）。
 * 仅声明本项目用到的成员：命令调用、事件订阅、当前窗口句柄。
 */

/** `invoke` 的参数对象。 */
interface TauriInvokeArgs {
  [key: string]: unknown;
}

/** `window.__TAURI__.core` 命名空间。 */
interface TauriCore {
  /** 调用 Rust command，失败时 promise 以错误字符串 reject。 */
  invoke<T>(cmd: string, args?: TauriInvokeArgs): Promise<T>;
}

/** 事件负载信封。 */
interface TauriEventPayload<T> {
  /** 事件负载。 */
  payload: T;
}

/** `window.__TAURI__.event` 命名空间。 */
interface TauriEvent {
  /** 订阅事件，返回取消订阅函数。 */
  listen<T>(event: string, handler: (event: TauriEventPayload<T>) => void): Promise<() => void>;
}

/** 当前窗口句柄。 */
interface TauriWindowHandle {
  /** 窗口标签（dashboard / main）。 */
  readonly label: string;
  /** 聚焦窗口。 */
  setFocus(): Promise<void>;
}

/** `window.__TAURI__` 全局对象。 */
interface TauriGlobal {
  /** 核心 API。 */
  core: TauriCore;
  /** 事件 API。 */
  event: TauriEvent;
  /** 窗口 API。 */
  window: {
    /** 获取当前窗口句柄。 */
    getCurrentWindow(): TauriWindowHandle;
  };
}

interface Window {
  /** Tauri 注入的全局 API。 */
  __TAURI__: TauriGlobal;
}
