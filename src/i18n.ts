const zh = {
  appTitle: "YouTube 下載",
  preparingTitle: "第一次使用，正在準備…",
  preparingStep: "第 {step} 步，共 {steps} 步",
  preparingHint: "大約需要 1 到 3 分鐘，請不要關掉視窗",
  preparingExtract: "正在整理檔案…（第 {step} 步，共 {steps} 步）",
  retry: "再試一次",
  readyPlaceholder: "準備好了",
  "error.network": "連不上網路，請檢查網路後再試一次",
  "error.tool_blocked": "下載工具被防毒軟體擋住了",
  "error.tools_missing": "準備失敗了",
  "error.github_busy": "下載來源暫時太忙，請過一小時再試",
  "error.disk_full": "電腦空間不夠了，請清出一些空間再試",
} as const;

export type MessageKey = keyof typeof zh;

export function t(key: MessageKey, params: Record<string, string | number> = {}): string {
  return Object.entries(params).reduce<string>(
    (text, [name, value]) => text.replace(`{${name}}`, String(value)),
    zh[key],
  );
}

export function errorMessage(code: string): string {
  const key = `error.${code}` as MessageKey;
  return key in zh ? t(key) : t("error.tools_missing");
}
