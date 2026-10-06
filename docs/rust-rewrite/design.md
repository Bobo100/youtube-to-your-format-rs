---
state: draft
created: 2026-10-07
supersedes: Bobo100/SideProject-Youtube-To-Your-Format (Electron 版 1.0.5)
---

# youtube-to-your-format 2.0:Tauri 2 + Rust 重寫

## 目的與成功標準

給**眼睛不好、不太會用電腦的長輩**用的 YouTube 下載 / 轉檔桌面程式(Windows)。舊版是 Electron 44 + Next 16 + 本機 Express,安裝檔約 260 MB,而且 YouTube 一改版就要 Bobo 重打包、到家人電腦上重裝。

使用者提出的四個動機(2026-10-07):

1. 家人不用 Bobo 再維護
2. 安裝檔變小、跑得輕
3. 練 Rust / Tauri
4. 介面重新設計得更簡單(不是照搬)

成功標準:

- 裝好之後,**常見的** YouTube 改版造成的下載失敗會自己修好:先更新 yt-dlp 穩定版,還不行就試 nightly 版。app 本身也會自己更新。Bobo 不用再去家人電腦重裝
  - 做不到的情況:yt-dlp 抬高了 JS runtime 的最低版本(見「工具」),要發新版 app 才能修。不過 app 會自己更新,所以 Bobo 只要發 release,不用到現場
- 安裝檔 ≤ 15 MB
  - 代價:第一次開啟要下載約 150–200 MB 的工具(ffmpeg、yt-dlp、Deno),所以要能**續傳**與**重試**
- 長輩不需要任何設定就能完成:打歌名或貼網址 → 按「存成音樂 / 存成影片」→ 打開資料夾
- 全程不會跳出黑色 console 視窗,也不會出現英文錯誤訊息
- 零伺服器:所有東西在本機跑,對外只讀 GitHub Releases 與 YouTube

## 範圍

| 功能 | 狀態 |
|---|---|
| 貼網址下載 MP3 / MP4 | 保留 |
| 搜尋 YouTube | 保留。**改用 yt-dlp `ytsearch`,不再需要 API 金鑰** |
| 本機檔案轉檔(MP3 / MP4) | 保留,格式跟舊版一樣只有這兩種 |
| 播放清單 / 一次排多首 | **新增** |
| yt-dlp 自動更新 + 失敗自動重試 | **新增** |
| app 自動更新 | **新增** |
| 字級設定(大 / 特大) | **新增** |
| 中文 / English、亮 / 暗 | 保留 |
| `cookies.txt`(需登入的影片) | 保留,給 Bobo 用的進階手段,長輩介面不提 |
| 舊版資料夾提示 | **新增**:偵測到舊的 `下載\youtube-downloads` 時,在主畫面提示一次「以前存的歌在這裡」 |
| YouTube Data API 金鑰、本機 Express server | 移除(搜尋改走 yt-dlp) |
| react-joyride 導覽 | 移除(介面簡單到不需要導覽) |
| 自訂輸出檔名 | 移除(檔名用影片標題;YAGNI) |
| 轉檔的其他格式(WAV、M4A…) | 不做(舊版也沒有;YAGNI) |
| 下載清單跨重開保留 | 不做(關掉 app 即清空;YAGNI) |

## 架構

新 repo `Bobo100/youtube-to-your-format-rs`,版本從 2.0.0 起。新版裝到家人電腦並穩定後:

1. 解除安裝舊版(列在 QA 清單)
2. 舊 repo 封存,README 指向新 repo

```
youtube-to-your-format-rs/
├─ src/                 前端:Vite + React + TypeScript(無 router、無 Redux)
└─ src-tauri/
   ├─ src/commands.rs   前端能呼叫的唯一介面(Tauri 這層只放這個)
   └─ core/src/         ytf-core:不依賴 Tauri 的邏輯，測試都在這裡
      ├─ process.rs     所有子行程的唯一出口:CREATE_NO_WINDOW、Job Object、UTF-8
      ├─ tools/         yt-dlp / ffmpeg / Deno 的安裝、更新、checksum、rollback
      ├─ ytdlp/         參數組裝、執行、進度解析、搜尋、播放清單展開、錯誤分類
      ├─ queue/         下載排隊(一次一個)、取消、狀態事件
      ├─ convert/       ffmpeg 本機轉檔
      └─ settings/      設定檔讀寫
```

Tauri 的「後端」是同一個程式裡的 Rust 程式碼,不是伺服器。前端是打包進 app 的網頁,用 Windows 內建的 WebView2 顯示。

### 子行程(`process.rs`)

Tauri app 是 GUI 程式。用 `std::process::Command` 直接開 yt-dlp / ffmpeg,預設會跳出黑色 console 視窗:長輩會被嚇到,也可能把它關掉。所以所有 spawn 都必須經過 `process.rs`,它負責四件事:

- 設 `CREATE_NO_WINDOW`
- 把子行程放進一個 Job Object,並設 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`。這樣取消時能結束整組子行程,app 當掉或被關掉時子行程也會一起消失。yt-dlp 會再開 ffmpeg,PyInstaller 也會再開 python 子行程,只殺 yt-dlp 不夠
- 加 `--encoding utf-8` 並設 `PYTHONUTF8=1`,避免中文標題經過 pipe 時被 cp950 弄壞
- spawn 失敗時,區分「檔案不見了」與「存取被拒」(多半是防毒隔離),交給錯誤分類

### 前後端契約(`commands.rs` + events)

| Command | 輸入 → 輸出 |
|---|---|
| `prepare_tools()` | 確保三個工具可用;過程發 `tools-progress` |
| `lookup(input, whole_playlist?)` | 網址或關鍵字 → `{ kind: "video" \| "playlist" \| "search", items: VideoCard[], truncated: bool }` |
| `enqueue(items, format)` | `format` = `audio` \| `video` → `JobId[]` |
| `cancel(job_id)` | 取消、結束 process tree、清掉暫存檔 |
| `convert(path, target)` | 本機檔 + `audio` \| `video` → `JobId`(與下載共用 Job 與事件) |
| `open_folder(path)` | 用檔案總管打開並選取檔案 |
| `get_settings()` / `set_settings(s)` | 讀寫設定 |
| `copy_diagnostics(job_id)` | 把錯誤資訊、工具版本與 hash、最近 log 放進剪貼簿 |

| Event | 內容 |
|---|---|
| `job-updated` | **整個 Job 快照**,前端用 `id` 直接取代。不送 diff,漏收一筆也不會不同步 |
| `tools-progress` | 第一次準備 / 背景更新的進度 |

- **`VideoCard`** = `{ id, url, title, channel, duration_s, thumbnail }`
  - 縮圖用 `https://i.ytimg.com/vi/<id>/mqdefault.jpg` 組出來,因為 flat 搜尋結果不保證帶縮圖
  - CSP 的 `img-src` 只放行 `i.ytimg.com`
- **`Job`** = `{ id, title, kind: download|convert, format, state, progress: number|null, output_path?, error? }`
  - `state` 依序是 `queued → downloading → processing → done`,另外有 `failed` / `canceled`
  - `processing`(合併影音、轉 mp3、轉檔前段)的 `progress` 是 `null`,前端顯示轉圈,因為 yt-dlp 的 FFmpeg postprocessor 不輸出進度
  - `error` 是分類代碼(見「錯誤處理」),文案由前端 i18n 決定
- 關閉視窗時,若還有 `queued` / `downloading` / `processing` 的 Job,要先問一次「還在下載,確定要關嗎?」

## 工具安裝與更新(`tools`)

三個工具都放在 `%LOCALAPPDATA%\youtube-to-your-format\bin\`。這個位置使用者可寫、不需要管理員權限,程式才能自己更新。第一次準備要逐檔下載,支援續傳與重試。

| 工具 | 為什麼需要 | 來源 | 更新方式 |
|---|---|---|---|
| **yt-dlp** | 下載 | GitHub `yt-dlp/yt-dlp` `releases/latest` 的 **onedir 版 `yt-dlp_win.zip`**(exe + `_internal/`,裝在 `bin\yt-dlp\`);stable 失敗時退到 `yt-dlp/yt-dlp-nightly-builds`。不用 onefile 的 `yt-dlp.exe`:它每次執行都解壓到 `%TEMP%\_MEI*`,被取消或逾時殺掉時這 24 MB 不會被清掉 | 自動 |
| **ffmpeg + ffprobe** | 合併影音、轉 mp3、轉檔、檢查影片編碼。`-x` 也需要 ffprobe | GyanD/codexffmpeg 的 essentials 7z(含 libx264,約 34 MB)。它從 2020 年起的每個有版本號的 release 都還在；BtbN 只保留最近 14 個 daily build,且 gpl 版 zip 要 184 MB | 固定版本：URL 與 SHA-256 寫死在程式裡，升級 = 改常數、發新版 app |
| **Deno** | yt-dlp 下載 YouTube 時需要外部 JS runtime 解 JS challenge,官方 yt-dlp.exe **不含** runtime。選 Deno 是因為 yt-dlp 預設啟用它,支援最完整 | `denoland/deno` 的 GitHub Release(會永久保留),固定版本 | 固定版本:URL 與 SHA-256 寫死。yt-dlp 抬高最低版本時要發新版 app |

yt-dlp 一律帶 `--js-runtimes deno:<bin 路徑>\deno.exe`,不依賴系統 PATH。

### yt-dlp 更新流程

1. 第一次開啟時下載。之後每次啟動,在背景檢查 `releases/latest`,最多一天一次
2. 下載 `yt-dlp_win.zip` 與同一個 release 的 `SHA2-256SUMS`,比對通過才解壓到 `bin\yt-dlp.new\`
3. 換檔前先取得 queue 的鎖,確保沒有 yt-dlp 正在執行。原因:Windows 不能覆蓋執行中的 exe
4. 換資料夾順序:`yt-dlp.new\` 解壓完成 → 現有的 rename 成 `yt-dlp.old\` → `.new` rename 成 `yt-dlp\`。中途斷電也不會留下壞的版本
5. 保留 `.old`。如果新版本身有 regression(例如換版後連第一個下載都失敗、舊版卻成功),就 rollback
6. `extractor` 錯誤的自動流程(見「錯誤處理」)會**強制**檢查,不受一天一次的限制

**已知風險**:同來源的 checksum 只能防傳輸損毀。一旦 yt-dlp 的 GitHub 帳號被盜,攻擊者就能在家人電腦上執行任意程式,而且不需要使用者做任何事。yt-dlp 另外有 GPG 簽章 `SHA2-256SUMS.sig`。第一版不驗,把「驗 GPG 簽章」列進 backlog。

### app 自我更新

- 使用 `tauri-plugin-updater`,讀 repo GitHub Releases 的 `latest.json`,用 updater 金鑰驗簽
- 安裝時機:Windows 上執行安裝步驟時 app 會被關掉,所以只在**啟動時、佇列還沒開始前**安裝。設 `installMode: "passive"`,只顯示進度條,不必按任何東西
- **updater 私鑰**:放 GitHub Actions secret,同時把私鑰與密碼**另外離線備份**,備份位置寫在 repo `CLAUDE.md`。私鑰遺失 = 已安裝的 app 永遠無法再更新
- **release 必須是正式(非 draft)且標為 latest**:tauri-action 預設建 draft,draft 期間 `releases/latest/download/latest.json` 拿不到
- 安裝檔不做 Authenticode 簽章:第一次安裝(由 Bobo 執行)會看到 SmartScreen,之後的自動更新不受影響

## 下載流程(`ytdlp` + `queue`)

### 1. `lookup`

- **非網址**:`ytsearch10:<關鍵字>`,加 `-J --flat-playlist`
- **網址含 `v=`**:即使也帶 `list=`(例如 Mix),預設加 `--no-playlist`,只回這一首;卡片下方多一顆「整個清單」按鈕,按了再用 `whole_playlist=true` 重新 lookup
- **純播放清單 / 頻道網址**:`-J --flat-playlist -I 1:200`。超過 200 首時設 `truncated=true`,畫面寫「只列出前 200 首」

### 2. 加入排隊

長輩在卡片上按「存成音樂 / 存成影片」,就把那首加入排隊。播放清單的最上面多一顆「全部存成音樂」→ `enqueue`。

### 3. 執行

- Worker **一次只跑一個**,狀態每變一次就發 `job-updated`
- 進度:`--newline --progress-template "download:%(progress)j"` 輸出 JSON,用 `downloaded_bytes` / `total_bytes`(或 `total_bytes_estimate`)計算
  - 影片有兩條 stream,`_percent_str` 會 0→100 跑兩次,所以 Rust 端把兩段合成一條進度
  - 下載結束後進入 `processing`

### 4. 檔名與路徑

- Rust 先算出唯一檔名:用影片標題,同名的話依序加 ` (2)`、` (3)`
- 用 `-o` 指定完整路徑(副檔名用最終格式),並加 `--no-overwrites --trim-filenames 150`
  - yt-dlp 遇到同名檔預設是跳過,不會自己編號
  - 中文長標題放在深層資料夾可能超過 MAX_PATH
- 用 `--print after_move:filepath` 取得實際輸出路徑,寫進 `output_path`,不用猜
- 輸出到設定的資料夾,預設 `下載\YouTube`

### 5. 取消

結束 Job Object,再刪掉這個 Job 的 `.part`、`.ytdl` 與中間檔,不在長輩的資料夾留下怪檔案。

### 格式

- **影片**:
  - `-f "bv*[vcodec^=avc1][height<=1080]+ba[ext=m4a]/b[ext=mp4]/bv*+ba/b" --merge-output-format mp4`
  - 下載完用 ffprobe 檢查影片編碼:是 H.264 就結束,**不重新編碼**(舊版每次都 `--recode-video`,很慢)
  - 不是 H.264 才用 ffmpeg 轉成 H.264 + AAC,確保 Windows 內建播放器與 LINE 能播。`--recode-video` 只看容器,VP9 放在 mp4 裡它不會轉,所以不能用
- **音樂**:`-f "ba/b" -x --audio-format mp3 --audio-quality 0 --embed-metadata`
- **cookies.txt**:照舊在 `~/cookies.txt`、`~/.config/yt-cookies/cookies.txt` 找,有就加 `--cookies`

## 轉檔(`convert`)

1. 拖檔或選檔 → 選「音樂 MP3 / 影片 MP4」→ 開始
2. 輸出在原檔旁邊:
   - `歌.wav` → `歌.mp3`
   - 原檔已經是目標格式時(`歌.mp4` → MP4),輸出 `歌 (轉檔).mp4`
   - 檔名衝突時一樣加 ` (2)`
3. 原檔所在資料夾不可寫(例如光碟)時,改存預設資料夾,並在卡片上寫明
4. 進度:先用 `ffprobe` 讀總長度,再用 `ffmpeg -nostats -progress pipe:1` 讀進度

## 錯誤處理

錯誤在 Rust 端分類成代碼,前端只顯示白話文案和能做的事。英文原文只進「複製問題資訊」與 log。

| 代碼 | 判斷 | 畫面 |
|---|---|---|
| `network` | 連線 / DNS / timeout | 「請檢查網路」+ 再試一次 |
| `unavailable` | 影片已刪除、私人、地區限制 | 「這部影片無法下載」 |
| `bot_check` | "Sign in to confirm you're not a bot" 之類 | 「YouTube 暫時擋住了,請過一陣子再試」。**不觸發**更新流程,也不叫長輩弄 cookies |
| `login_required` | 會員限定、年齡限制 | 「這部影片要登入才能下載」 |
| `format_unavailable` | "Requested format is not available" | 「這部影片沒有可下載的版本」。**不觸發**更新流程 |
| `disk_full` | 寫檔失敗且空間不足 | 「電腦空間不夠了」 |
| `tool_blocked` | spawn 時存取被拒,或工具檔案不見了(多半是防毒隔離) | 「下載工具被防毒軟體擋住了」+「複製問題資訊(傳給 Bobo)」+「重新準備」 |
| `tools_missing` | 第一次準備失敗 | 「準備失敗」+ 再試一次(續傳);不讓家人卡在半殘狀態 |
| `extractor` | 其他(多半是 YouTube 改版) | 自動流程,見下方 |

`extractor` 的自動流程:

1. 強制檢查 yt-dlp stable,有新版就更新並重試一次
2. 還是失敗 → 改抓 nightly 並重試一次
3. 仍然失敗 → 顯示「這首現在存不下來,過一兩天再試試看」,附「再試一次」與「複製問題資訊(傳給 Bobo)」兩顆按鈕

其他情況:

- 輸出資料夾不存在(例如隨身碟拔掉)→ 改存預設資料夾,卡片寫明存到哪
- app 自我更新失敗 → 不打擾,寫 log,下次啟動再試
- 設定檔損毀 → 用預設值,並備份壞檔
- Log:放在 `%LOCALAPPDATA%\youtube-to-your-format\logs\`,輪替保留最近 5 個檔;每次啟動記錄三個工具的版本與 SHA-256

## 介面

設計依據是 `ui-ux-pro-max` 的 Accessible 風格:WCAG AAA 對比、單欄、每個畫面一個主要動作。Mockup 在 [docs/mockups/ui/](../mockups/ui/README.md)。

- **單頁主畫面**:
  - 「想下載什麼?」大輸入框,打歌名或貼網址都走同一套流程
  - 結果是影片卡片(縮圖、標題、時長),每張卡片兩顆按鈕「存成音樂 / 存成影片」
  - **不先選 MP3 / MP4**:按鈕上的字就是動作,沒有要記住的狀態
- **進度**:按下後進度顯示在**同一張卡片**上。完成時顯示「好了!已存到『下載 › YouTube』」和「打開資料夾」
- **轉檔、設定**:放在右上角,圖示一律配文字
- **規格**:
  - 字級兩檔:大 18px、**特大 20px(預設)**
  - 按鈕高 52–60px,間距 ≥ 10px
  - 內文 `#0B1220` 在 `#F8FAFC` 上
  - 焦點框 3px
  - 圖示用 SVG(Lucide 風格),不用 emoji
- **顏色**:**預設亮色**、不跟系統,因為亮底深字對長輩較好讀;可在設定改暗色
- **設定**:只有 4 項:存到哪裡、字的大小、畫面顏色、語言(預設中文)
- **文案**:全用台灣口語,不出現「格式」「編碼」「API」之類的詞

## 測試

### Rust 單元測試(主力,`cargo test`)

- 進度 JSON 解析,含兩條 stream 合併
- 錯誤分類:fixture 用真實的 yt-dlp stderr 樣本,涵蓋表中每個代碼
- yt-dlp / ffmpeg 參數組裝,含 `v=` + `list=` 的網址判斷
- 唯一檔名
- `SHA2-256SUMS` 解析與驗證
- 版本比較
- 換檔與 rollback 的檔案操作
- 設定檔損毀時的 fallback

### 整合測試

標成 `#[ignore]`,手動跑 `cargo test -- --ignored`:

- 真的下載一部短的 CC 授權影片,音樂與影片各一次
- yt-dlp 更新流程

### 前端 Vitest

- 網址 / 關鍵字判斷
- Job 狀態 → 文案對應

### 手動 QA 清單

寫在 repo `CLAUDE.md`,在沒有管理員權限的 Windows 帳號上跑:

1. 安裝
2. 第一次準備,中途斷網再恢復,確認能續傳
3. 全程沒有出現 console 視窗
4. 用注音輸入搜尋
5. 特大字級
6. 下載含 `list=` 的網址:預設只下一首
7. 下載播放清單
8. 下載中取消:資料夾裡沒有殘檔
9. 下載中關閉視窗:會先問
10. 放舊版 yt-dlp 模擬 YouTube 改版,確認會自動更新後重試
11. app 自我更新
12. 解除安裝舊版 Electron app,確認會出現舊資料夾提示

### CI

GitHub Actions,Windows runner:

- PR:`cargo test` + 前端 test + lint
- 打 tag:release workflow 產出 NSIS 安裝檔與簽章的 `latest.json`,並發布成**正式**、標為 latest 的 release

## 未決 / 實作時確認

- `--progress-template` 在影片兩條 stream 時的實際輸出,用真實輸出建立 fixture
- Windows 10 舊機若沒有 WebView2,由 NSIS 用 `embedBootstrapper` 安裝
- Backlog:驗證 yt-dlp 的 GPG 簽章
