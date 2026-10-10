/**
 * Tauri 全局注入 API 的最小类型声明（tauri.conf.json 中 app.withGlobalTauri = true）。
 * 本切片只用到窗口句柄的 label，其余 API 由后续切片按需补充。
 */

/** 当前窗口句柄（仅声明本切片用到的成员）。 */
interface TauriWindowHandle {
  /** 窗口标签（dashboard / main）。 */
  readonly label: string;
  /** 聚焦窗口。 */
  setFocus(): Promise<void>;
}

/** `window.__TAURI__` 全局对象（最小声明）。 */
interface TauriGlobal {
  window: {
    /** 获取当前窗口句柄。 */
    getCurrentWindow(): TauriWindowHandle;
  };
}

interface Window {
  /** Tauri 注入的全局 API。 */
  __TAURI__: TauriGlobal;
}
