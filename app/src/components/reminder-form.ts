/**
 * 新增/编辑提醒表单（仪表盘与主窗口共用组件函数）。
 * 垂直单列布局：每字段 = 标签(11/600 text-dim 置顶) + 控件 + 行内错误位；
 * 三组单选（重复/排程/触达）用分段控件，次级配置统一 .sub-group 缩进。
 * 保存前本地校验模型不变量（错误红字内联）；保存携带表单打开时的 rev 快照，
 * 过期保存（SaveConflict）弹「数据已被外部修改，请重试」并重载；提交期间禁用保存按钮防重入。
 */
import {
  addReminder,
  fetchSettings,
  fetchStoreRev,
  formatError,
  isSaveConflictError,
  updateReminder,
  type ReminderDetail,
  type ReminderPayload,
} from "../api";
import { alertDialog } from "./dialog";

/** 表单选项。 */
export interface ReminderFormOptions {
  /** 编辑时的初值；缺省为新增默认值。 */
  initial?: ReminderDetail;
  /** 保存成功或保存冲突后收尾（调用方切回列表并重载）。 */
  onFinished: () => void;
  /** 取消回调。 */
  onCancel: () => void;
}

/** 星期短名与中文标签（周一…周日）。 */
const WEEKDAYS: ReadonlyArray<[string, string]> = [
  ["mon", "一"],
  ["tue", "二"],
  ["wed", "三"],
  ["thu", "四"],
  ["fri", "五"],
  ["sat", "六"],
  ["sun", "日"],
];

/** 行内错误段（空文案 = 隐藏）。 */
function errorLine(): HTMLParagraphElement {
  const line = document.createElement("p");
  line.className = "field-error";
  return line;
}

/** 表单小节标题（11/600 text-dim + 字距；「重复」「排程」「触达」分组分隔）。 */
function groupTitle(text: string): HTMLParagraphElement {
  const title = document.createElement("p");
  title.className = "group-title";
  title.textContent = text;
  return title;
}

/**
 * 追加一个分段控件项（单选），返回其 input（checked 由调用方读写）。
 * 容器应为 .segmented；项视觉与 chip 选中态一致（surface-raised 底 + text 字）。
 */
function appendSegment(
  container: HTMLElement,
  name: string,
  text: string,
  checked: boolean,
): HTMLInputElement {
  const label = document.createElement("label");
  label.className = "radio";
  const input = document.createElement("input");
  input.type = "radio";
  input.name = name;
  input.checked = checked;
  const span = document.createElement("span");
  span.textContent = text;
  label.append(input, span);
  container.append(label);
  return input;
}

/** 追加一行「标签 + 控件 + 行内错误位」的垂直字段行。 */
function appendField(
  container: HTMLElement,
  label: string,
  control: HTMLElement,
  error: HTMLElement,
): void {
  const row = document.createElement("div");
  row.className = "form-row";
  const caption = document.createElement("label");
  caption.className = "form-label";
  caption.textContent = label;
  row.append(caption, control, error);
  container.append(row);
}

/** 解析 HH:MM（兼容 H:MM 与 HH:MM:SS）→ 归一化 HH:MM；非法返回 null。 */
function normalizeTime(raw: string): string | null {
  const match = /^(\d{1,2}):(\d{2})(?::(\d{2}))?$/.exec(raw.trim());
  if (match === null) {
    return null;
  }
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  if (hour > 23 || minute > 59) {
    return null;
  }
  return `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`;
}

/** 逗号/顿号/空白分隔的时刻串 → 归一化列表；含非法项返回 null。 */
function parseTimeList(raw: string): string[] | null {
  const times: string[] = [];
  for (const token of raw.split(/[,，;；\s]+/)) {
    if (token === "") {
      continue;
    }
    const normalized = normalizeTime(token);
    if (normalized === null) {
      return null;
    }
    times.push(normalized);
  }
  return times;
}

/** 构建新增/编辑表单。 */
export function reminderForm(options: ReminderFormOptions): HTMLElement {
  const initial = options.initial;
  const root = document.createElement("form");
  root.className = "reminder-form";

  // —— 表单会话持有的 store rev 快照（保存基线，防过期快照覆盖）：
  // 编辑取 get 时的 rev；新增挂载时取一次当前 rev
  let baseRev: number | undefined = initial?.rev;
  if (baseRev === undefined) {
    void fetchStoreRev()
      .then((rev) => {
        baseRev = rev;
      })
      .catch(() => {
        // 取不到快照就退回按当前 rev 保存，不阻塞表单
      });
  }

  // —— 头部：标题 + ✕ 关闭（取消同义）
  const head = document.createElement("div");
  head.className = "form-head";
  const headTitle = document.createElement("p");
  headTitle.className = "form-title";
  headTitle.textContent = initial === undefined ? "新增提醒" : "编辑提醒";
  const closeButton = document.createElement("button");
  closeButton.type = "button";
  closeButton.className = "icon-button form-close";
  closeButton.textContent = "✕";
  closeButton.setAttribute("aria-label", "关闭");
  head.append(headTitle, closeButton);

  // —— 主体（可滚动字段列）
  const body = document.createElement("div");
  body.className = "form-body";

  // —— 标题 / 备注
  const titleInput = document.createElement("input");
  titleInput.type = "text";
  titleInput.className = "input";
  titleInput.placeholder = "例如：提肛运动";
  titleInput.value = initial?.title ?? "";
  const titleError = errorLine();
  const noteInput = document.createElement("textarea");
  noteInput.className = "input";
  noteInput.rows = 2;
  noteInput.placeholder = "备注（可空）";
  noteInput.value = initial?.note ?? "";

  // —— 重复：每天 / 每周 分段控件 + 星期多选 chip
  const repeatBox = document.createElement("div");
  repeatBox.className = "segmented";
  const repeatDaily = appendSegment(repeatBox, "repeat", "每天", initial?.repeat !== "weekly");
  const repeatWeekly = appendSegment(repeatBox, "repeat", "每周", initial?.repeat === "weekly");
  const weekdayBox = document.createElement("div");
  weekdayBox.className = "chip-row sub-group";
  const weekdayInputs: HTMLInputElement[] = [];
  for (const [value, label] of WEEKDAYS) {
    const chip = document.createElement("label");
    chip.className = "chip";
    const input = document.createElement("input");
    input.type = "checkbox";
    input.value = value;
    input.checked = initial?.weekdays.includes(value) ?? false;
    const text = document.createElement("span");
    text.textContent = label;
    chip.append(input, text);
    weekdayBox.append(chip);
    weekdayInputs.push(input);
  }
  const repeatError = errorLine();
  const repeatSection = document.createElement("div");
  repeatSection.className = "form-section";
  repeatSection.append(groupTitle("重复"), repeatBox, weekdayBox, repeatError);

  // —— 排程：每隔 / 固定时刻 分段控件
  const modeBox = document.createElement("div");
  modeBox.className = "segmented";
  const modeEvery = appendSegment(
    modeBox,
    "schedule-mode",
    "每隔",
    initial?.schedule_mode !== "at",
  );
  const modeAt = appendSegment(
    modeBox,
    "schedule-mode",
    "固定时刻",
    initial?.schedule_mode === "at",
  );

  // —— 排程·每隔：成对字段行（间隔+单位 / 时间窗起–止）
  const everyBox = document.createElement("div");
  everyBox.className = "sub-group";
  const intervalInput = document.createElement("input");
  intervalInput.type = "number";
  intervalInput.className = "input w-interval";
  intervalInput.min = "1";
  intervalInput.step = "1";
  const unitSelect = document.createElement("select");
  unitSelect.className = "input w-unit";
  const unitMinute = document.createElement("option");
  unitMinute.value = "minute";
  unitMinute.textContent = "分钟";
  const unitHour = document.createElement("option");
  unitHour.value = "hour";
  unitHour.textContent = "小时";
  unitSelect.append(unitMinute, unitHour);
  if (initial !== undefined && initial.schedule_mode !== "at") {
    if (initial.interval_minutes % 60 === 0 && initial.interval_minutes >= 60) {
      intervalInput.value = String(initial.interval_minutes / 60);
      unitSelect.value = "hour";
    } else {
      intervalInput.value = String(initial.interval_minutes);
    }
  } else {
    intervalInput.value = "2";
    unitSelect.value = "hour";
  }
  const intervalRow = document.createElement("div");
  intervalRow.className = "pair-row";
  const intervalLabel = document.createElement("span");
  intervalLabel.className = "mini-label";
  intervalLabel.textContent = "间隔";
  const intervalControls = document.createElement("div");
  intervalControls.className = "pair-controls";
  intervalControls.append(intervalInput, unitSelect);
  intervalRow.append(intervalLabel, intervalControls);

  const windowStartInput = document.createElement("input");
  windowStartInput.type = "time";
  windowStartInput.className = "input w-time";
  windowStartInput.value = initial?.window_start || "09:00";
  const windowEndInput = document.createElement("input");
  windowEndInput.type = "time";
  windowEndInput.className = "input w-time";
  windowEndInput.value = initial?.window_end || "18:00";
  const windowRow = document.createElement("div");
  windowRow.className = "pair-row";
  const windowLabel = document.createElement("span");
  windowLabel.className = "mini-label";
  windowLabel.textContent = "时间窗";
  const windowControls = document.createElement("div");
  windowControls.className = "pair-controls";
  const startLabel = document.createElement("label");
  startLabel.className = "mini-sublabel";
  const startText = document.createElement("span");
  startText.textContent = "开始";
  startLabel.append(startText, windowStartInput);
  const dash = document.createElement("span");
  dash.className = "pair-dash";
  dash.textContent = "–";
  const endLabel = document.createElement("label");
  endLabel.className = "mini-sublabel";
  const endText = document.createElement("span");
  endText.textContent = "结束";
  endLabel.append(endText, windowEndInput);
  windowControls.append(startLabel, dash, endLabel);
  windowRow.append(windowLabel, windowControls);
  everyBox.append(intervalRow, windowRow);

  // —— 排程·固定时刻
  const atBox = document.createElement("div");
  atBox.className = "sub-group";
  const timesInput = document.createElement("input");
  timesInput.type = "text";
  timesInput.className = "input";
  timesInput.placeholder = "9:00,14:30";
  timesInput.value = initial?.times.join(",") || "9:00";
  const atRow = document.createElement("div");
  atRow.className = "pair-row";
  const atLabel = document.createElement("span");
  atLabel.className = "mini-label";
  atLabel.textContent = "时刻";
  const atControls = document.createElement("div");
  atControls.className = "pair-controls";
  atControls.append(timesInput);
  atRow.append(atLabel, atControls);
  atBox.append(atRow);

  const scheduleError = errorLine();
  const scheduleSection = document.createElement("div");
  scheduleSection.className = "form-section";
  scheduleSection.append(groupTitle("排程"), modeBox, everyBox, atBox, scheduleError);

  // —— 触达：系统通知 / 弹窗 分段控件
  const deliveryBox = document.createElement("div");
  deliveryBox.className = "segmented";
  const deliveryNotification = appendSegment(
    deliveryBox,
    "delivery",
    "系统通知",
    initial?.delivery !== "popup",
  );
  const deliveryPopup = appendSegment(
    deliveryBox,
    "delivery",
    "弹窗",
    initial?.delivery === "popup",
  );

  // —— 触达·系统通知：声音
  const notificationBox = document.createElement("div");
  notificationBox.className = "sub-group";
  const soundLabel = document.createElement("label");
  soundLabel.className = "checkbox";
  const soundInput = document.createElement("input");
  soundInput.type = "checkbox";
  soundInput.checked = initial?.sound ?? true;
  soundLabel.append(soundInput, document.createTextNode("声音"));
  notificationBox.append(soundLabel);

  // —— 触达·弹窗：自动消失（秒数）/ 强制处理
  const popupBox = document.createElement("div");
  popupBox.className = "sub-group";
  const popupModes = document.createElement("div");
  popupModes.className = "chip-row";
  const popupAuto = appendSegment(
    popupModes,
    "popup-mode",
    "自动消失",
    initial?.popup_mode !== "force",
  );
  const popupForce = appendSegment(
    popupModes,
    "popup-mode",
    "强制处理",
    initial?.popup_mode === "force",
  );
  const autoRow = document.createElement("div");
  autoRow.className = "pair-row";
  const autoLabel = document.createElement("span");
  autoLabel.className = "mini-label";
  autoLabel.textContent = "自动消失";
  const autoControls = document.createElement("div");
  autoControls.className = "pair-controls";
  const autoSecondsInput = document.createElement("input");
  autoSecondsInput.type = "number";
  autoSecondsInput.className = "input w-interval";
  autoSecondsInput.min = "1";
  autoSecondsInput.step = "1";
  autoSecondsInput.value = String(initial?.auto_dismiss_seconds ?? 30);
  const autoUnit = document.createElement("span");
  autoUnit.className = "dim-suffix";
  autoUnit.textContent = "秒";
  autoControls.append(autoSecondsInput, autoUnit);
  autoRow.append(autoLabel, autoControls);
  popupBox.append(popupModes, autoRow);

  const deliveryError = errorLine();
  const deliverySection = document.createElement("div");
  deliverySection.className = "form-section";
  deliverySection.append(groupTitle("触达"), deliveryBox, notificationBox, popupBox, deliveryError);

  // —— 稍后分钟数 / 错过补发
  const snoozeInput = document.createElement("input");
  snoozeInput.type = "number";
  snoozeInput.className = "input w-interval";
  snoozeInput.min = "1";
  snoozeInput.step = "1";
  snoozeInput.value = String(initial?.snooze_minutes ?? 10);
  const snoozeControls = document.createElement("div");
  snoozeControls.className = "pair-controls";
  const snoozeUnit = document.createElement("span");
  snoozeUnit.className = "dim-suffix";
  snoozeUnit.textContent = "分钟";
  snoozeControls.append(snoozeInput, snoozeUnit);
  const catchUpLabel = document.createElement("label");
  catchUpLabel.className = "checkbox";
  const catchUpInput = document.createElement("input");
  catchUpInput.type = "checkbox";
  catchUpInput.checked = initial?.catch_up ?? false;
  catchUpLabel.append(catchUpInput, document.createTextNode("错过补发（下次启动补触发错过时刻）"));

  // —— 提交错误（后端非冲突错误）与底部操作栏（sticky：左取消 ghost / 右保存 primary）
  const submitError = errorLine();
  const actions = document.createElement("div");
  actions.className = "form-actions";
  const saveButton = document.createElement("button");
  saveButton.type = "submit";
  saveButton.className = "button primary";
  saveButton.textContent = "保存";
  const cancelButton = document.createElement("button");
  cancelButton.type = "button";
  cancelButton.className = "button ghost";
  cancelButton.textContent = "取消";
  actions.append(cancelButton, saveButton);

  // —— 子区块显隐同步
  const syncVisibility = (): void => {
    weekdayBox.hidden = !repeatWeekly.checked;
    everyBox.hidden = !modeEvery.checked;
    atBox.hidden = !modeAt.checked;
    notificationBox.hidden = !deliveryNotification.checked;
    popupBox.hidden = !deliveryPopup.checked;
    autoRow.hidden = !popupAuto.checked;
    autoSecondsInput.disabled = !popupAuto.checked;
  };
  for (const control of [
    repeatDaily,
    repeatWeekly,
    modeEvery,
    modeAt,
    deliveryNotification,
    deliveryPopup,
    popupAuto,
    popupForce,
  ]) {
    control.addEventListener("change", syncVisibility);
  }
  syncVisibility();

  // —— 「默认声音」设置预填（仅新增且用户未动过勾选时）
  if (initial === undefined) {
    let soundTouched = false;
    soundInput.addEventListener("change", () => {
      soundTouched = true;
    });
    void fetchSettings()
      .then((settings) => {
        if (!soundTouched) {
          soundInput.checked = settings.default_sound;
        }
      })
      .catch(() => {
        // 读不到设置就保持默认勾选，不阻塞表单
      });
  }

  /** 显示行内错误；返回是否有错误。 */
  const showErrors = (): boolean => {
    let hasError = false;
    for (const [line, message] of [
      [titleError, titleError.textContent ?? ""],
      [repeatError, repeatError.textContent ?? ""],
      [scheduleError, scheduleError.textContent ?? ""],
      [deliveryError, deliveryError.textContent ?? ""],
    ] as Array<[HTMLElement, string]>) {
      line.textContent = message;
      if (message !== "") {
        hasError = true;
      }
    }
    return hasError;
  };

  /** 本地校验并组装载荷；非法时写好各错误位并返回 null。 */
  const collectPayload = (): ReminderPayload | null => {
    titleError.textContent = titleInput.value.trim() === "" ? "标题不能为空" : "";
    const weekdays = weekdayInputs.filter((input) => input.checked).map((input) => input.value);
    repeatError.textContent =
      repeatWeekly.checked && weekdays.length === 0 ? "每周重复至少需要指定一个星期" : "";

    let intervalMinutes = 0;
    let windowStart = "";
    let windowEnd = "";
    let times: string[] = [];
    if (modeEvery.checked) {
      const value = intervalInput.valueAsNumber;
      intervalMinutes = Math.trunc(value * (unitSelect.value === "hour" ? 60 : 1));
      windowStart = windowStartInput.value;
      windowEnd = windowEndInput.value;
      const start = normalizeTime(windowStart);
      const end = normalizeTime(windowEnd);
      if (!Number.isFinite(value) || intervalMinutes < 1) {
        scheduleError.textContent = "间隔分钟数必须 ≥ 1";
      } else if (start === null || end === null) {
        scheduleError.textContent = "时间窗起止格式应为 HH:MM";
      } else if (end <= start) {
        scheduleError.textContent = "时间窗终点必须晚于起点";
      } else {
        scheduleError.textContent = "";
        windowStart = start;
        windowEnd = end;
      }
    } else {
      const parsed = parseTimeList(timesInput.value);
      if (parsed === null) {
        scheduleError.textContent = "固定时刻格式应为 HH:MM（如 9:00,14:30）";
      } else if (parsed.length === 0) {
        scheduleError.textContent = "固定时刻列表不能为空";
      } else {
        scheduleError.textContent = "";
        times = parsed;
      }
    }

    if (showErrors()) {
      return null;
    }
    return {
      title: titleInput.value.trim(),
      note: noteInput.value.trim() === "" ? null : noteInput.value.trim(),
      repeat: repeatWeekly.checked ? "weekly" : "daily",
      weekdays,
      schedule_mode: modeEvery.checked ? "every" : "at",
      interval_minutes: intervalMinutes,
      window_start: windowStart,
      window_end: windowEnd,
      times,
      delivery: deliveryNotification.checked ? "notification" : "popup",
      sound: soundInput.checked,
      popup_mode: popupForce.checked ? "force" : "auto",
      auto_dismiss_seconds: Math.max(1, Math.trunc(autoSecondsInput.valueAsNumber || 30)),
      snooze_minutes: Math.max(1, Math.trunc(snoozeInput.valueAsNumber || 10)),
      catch_up: catchUpInput.checked,
    };
  };

  // —— 提交：防重入（提交期间禁用按钮，完成或失败后恢复）
  let submitting = false;
  root.addEventListener("submit", (event) => {
    event.preventDefault();
    if (submitting) {
      return;
    }
    submitError.textContent = "";
    const payload = collectPayload();
    if (payload === null) {
      return;
    }
    submitting = true;
    saveButton.disabled = true;
    const saving =
      initial === undefined
        ? addReminder(payload, baseRev)
        : updateReminder(initial.id, payload, initial.rev);
    void saving
      .then(() => {
        options.onFinished();
      })
      .catch((err: unknown) => {
        if (isSaveConflictError(err)) {
          void alertDialog("数据已被外部修改，请重试").then(() => options.onFinished());
        } else {
          submitError.textContent = formatError(err);
        }
      })
      .finally(() => {
        submitting = false;
        saveButton.disabled = false;
      });
  });
  cancelButton.addEventListener("click", () => options.onCancel());
  closeButton.addEventListener("click", () => options.onCancel());

  appendField(body, "标题", titleInput, titleError);
  appendField(body, "备注", noteInput, errorLine());
  body.append(repeatSection, scheduleSection, deliverySection);
  appendField(body, "稍后提醒", snoozeControls, errorLine());
  const catchUpRow = document.createElement("div");
  catchUpRow.className = "form-row";
  catchUpRow.append(catchUpLabel);
  body.append(catchUpRow, submitError);

  root.append(head, body, actions);
  return root;
}
