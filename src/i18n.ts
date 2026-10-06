const zh = {
  appTitle: "YouTube 下載",
  preparingTitle: "第一次使用，正在準備…",
  preparingStep: "第 {step} 步，共 {steps} 步",
  preparingHint: "大約需要 1 到 3 分鐘，請不要關掉視窗",
  preparingExtract: "正在整理檔案…（第 {step} 步，共 {steps} 步）",
  retry: "再試一次",
  homeLabel: "想下載什麼？",
  homeHelp: "打歌名，或把 YouTube 網址貼上來",
  find: "找",
  finding: "正在找…",
  noResults: "找不到，換幾個字試試看",
  saveAudio: "存成音樂",
  saveVideo: "存成影片",
  saveAllAudio: "全部存成音樂",
  wholePlaylist: "整個清單都要",
  playlistCount: "共 {count} 首",
  playlistTruncated: "清單太長，只列出前 {count} 首",
  skippedCount: "有 {count} 首是私人、會員限定或直播，沒辦法下載，已略過",
  foundCount: "找到 {count} 個結果",
  whatAudio: "音樂",
  whatVideo: "影片",
  jobQueued: "排隊中，等前面的下載完…",
  jobDownloading: "正在下載{what}… {percent}%",
  jobDownloadingShort: "正在下載{what}",
  "error.open_failed": "打不開資料夾",
  jobProcessing: "快好了，正在處理…",
  jobUpdating: "YouTube 好像改版了，正在更新下載工具，請稍等…",
  copyDiagnostics: "複製問題資訊（傳給 Bobo）",
  copied: "已複製，可以貼到 LINE 傳給 Bobo",
  jobDone: "好了！已存到「下載 › YouTube」",
  jobCanceled: "已取消",
  jobFailed: "沒有下載成功",
  cancel: "取消",
  openFolder: "打開資料夾",
  otherJobs: "其他下載",
  "error.lookup_failed": "找不到這部影片，請確認網址是否正確",
  "error.search_failed": "現在搜尋不到，請過一會兒再試",
  "error.unavailable": "這部影片沒辦法下載（可能是直播、私人或會員限定）",
  "error.not_youtube": "這不是 YouTube 的網址",
  "error.empty_input": "請先打字或貼上網址",
  "error.download_failed": "這首現在存不下來，請過一兩天再試試看",
  "error.extractor": "這首現在存不下來，請過一兩天再試試看",
  "error.bot_check": "YouTube 暫時擋住了，請過一陣子再試",
  "error.login_required": "這部影片要登入才能下載",
  "error.format_unavailable": "這部影片沒有可以下載的版本",
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

export function errorMessage(code: string, fallback: MessageKey = "error.tools_missing"): string {
  const key = `error.${code}` as MessageKey;
  return key in zh ? t(key) : t(fallback);
}
