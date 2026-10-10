---
state: in-progress
updated: 2026-10-10
plan_format: 2
---

# Rust 重寫 — Plan

**Goal:** 長輩在自己的 Windows 電腦上打歌名或貼網址就能存成音樂 / 影片;YouTube 常見的改版與 app 更新都自動處理,Bobo 不用再到現場重裝。

**Scope:** [design.md](design.md) 的全部範圍。2.0.0 裝到家人電腦後，舊 Electron repo 封存。

**Design:** [design.md](design.md)、mockup [docs/mockups/ui/](../mockups/ui/README.md)

每個項目一個 PR(`type(scope): 中文描述`,commit subject 結尾加 `(rust-rewrite/W-NN)`),PR merge 前在 branch 上寫好該項目的 `Done:` 與證據。

## Phase 1 — 能用的 app

Checkpoint: debug exe 可以搜尋、下載 MP3 / MP4、轉本機檔案、改設定;壞掉的 yt-dlp 會自動更新後重試。

### W-01 — 專案骨架與 CI

- [x] GitHub repo `Bobo100/youtube-to-your-format-rs`(public,同舊 repo)
- [x] Tauri 2 + Vite + React + TypeScript 骨架,app 名稱、identifier、視窗大小;NSIS currentUser 安裝
- [x] `CLAUDE.md`(架構心智模型、鐵則：子行程只走 `process.rs`、doc map、commit 格式、手動 QA 清單)+ `AGENTS.md` 指向 `CLAUDE.md`
- [x] Vitest、ESLint、`cargo test` / `clippy`;GitHub Actions Windows runner 在 PR 上跑
- [x] Verify: CI 綠燈;`npm run tauri dev` 開出空視窗

Evidence: PR #2。本機 `npm run lint`、`npm test`(1 passed)、`npm run build`、`npm run rs:clippy`、`npm run rs:test` 通過;CI run 37511365158 check pass(6m21s);debug exe 啟動後視窗標題「YouTube 下載」、中文正常(截圖)。
Deviation: 驗證視窗原本用 `npm run tauri dev` → 改用 `npm run rs:build-debug` 啟動內嵌前端的 debug exe → 不必另開 dev server → 同上截圖。另外本機沒有 MSVC build tools,Rust 指令改走 GNU toolchain wrapper(沿用 bai-e-desktop-pet),lib 只留 `rlib`(windows-gnu 下 cdylib 超過 ld 的 export ordinal 上限);CI / release 仍用 MSVC。

Done: PR #2 · verify: `npm test` and `npm run rs:test` → pass · note: CI run 37511365158 pass;debug exe 開出視窗

### W-02 — 子行程與工具安裝(`process.rs`、`tools`)

- [x] `process.rs`:`CREATE_NO_WINDOW`、Job Object + `KILL_ON_JOB_CLOSE`、UTF-8、區分「不見了 / 被拒」
- [x] ffmpeg:選定固定版本、確認含 ffprobe(removed from scope: 重新上傳到 `tools-ffmpeg-<版本>` → 改直接用 GyanD,見 Deviation)
- [x] Deno:選定版本並確認符合 yt-dlp 最低需求
- [x] 三個工具的下載(續傳、重試)、SHA-256 驗證、解壓到 `%LOCALAPPDATA%\youtube-to-your-format\bin\`
- [x] `--js-runtimes` 指向 Deno — W-03 PR #4 用 `ytdlp::base_args` 做好
- [x] 第一次準備畫面 + `tools-progress`
- [x] Verify: 單元測試(SHA2-256SUMS 解析、驗證；版本比較移到 W-05);手動：刪掉 `bin\` 後重開會重新準備，中途斷網可續傳;全程無 console 視窗

Evidence: PR #3。`npm run rs:test` 27 passed,涵蓋:
- process:隱藏子行程的輸出、Missing 分類、防毒錯誤碼 5 / 225 / 226 判成 Blocked、kill_tree、卡住的子行程逾時後整組結束
- 工具安裝:SHA2-256SUMS 解析、hash 錯就刪掉 `.part`、`.part` 以 hash 命名並清掉舊版本的、4xx 不重試、換檔保留 `.old` 與 rollback、zip 解壓與同名去重、`state.json` 原子寫入與損毀 fallback、`needed()` 判斷、錯誤代碼對應

其他驗證:
- `-- --ignored` 2 passed:真的把三個工具裝到 temp dir,並各自跑 `--version`(17.1s);先放 3 MB `.part` 再下載，第一筆進度 > 3 MB,證明 Range 續傳
- `npm test` 5 passed
- 手動：刪掉 `bin\` 後啟動 debug exe,顯示準備畫面(截圖),完成後顯示「準備好了」;`bin\` 只剩三個工具和 `state.json`,沒有殘留的暫存檔
- 準備過程中列出所有有視窗的 process,沒有 yt-dlp / ffmpeg / deno / conhost
- 第二次啟動時，第二個 process 會自己結束(single-instance)

對抗性 review 修正：
- 加 single-instance
- 驗證工具時加 60s timeout
- 子行程先 suspended,放進 Job Object 後才 resume
- `SetErrorMode`,缺 DLL 時直接失敗，不跳出看不到的對話框
- 新增錯誤代碼 github_busy / disk_full / 防毒錯誤碼
- 改名遇到防毒鎖檔會重試
- 驗證失敗就 rollback
- `.part` 以 hash 命名
- 檢查 `Content-Range` 的起點
- 前端先註冊 listener 再開始準備

L7(整體下載時限)沒修，寫進 vault backlog。
Deviation:
- ffmpeg 來源:原本把 BtbN build 重新上傳到自己 repo 的 release → 改成直接下載 GyanD/codexffmpeg 9.0.2 essentials 7z → BtbN gpl 版 zip 要 184 MB;GyanD 只有 34 MB,而且從 2020 年起每個有版本號的 release 都還在 → hash 對過 gyan.dev 公布的 `.sha256`。
- 版本比較:原本放在 W-02 → 移到 W-05 → 只有 yt-dlp 更新會用到。
- 「中途斷網」:原本手動拔網路測 → 改用強制結束 app 再重開，加上 Range 續傳的整合測試 → 兩者都會留下 `.part`,走的是同一條路徑。
- 架構:原本把 `process.rs`、`tools/` 放在 `src-tauri/src/` → 改放進新的 workspace crate `ytf-core`,Tauri crate 設 `test = false` → 連結 Tauri 的測試執行檔一跑就 `STATUS_ENTRYPOINT_NOT_FOUND` → design / CLAUDE.md 已同步更新。

Done: PR #3 · verify: `npm test`, `npm run rs:test` and `npm run rs:test -- --ignored` → pass · note: 手動刪 bin 後重新準備、全程無 console 視窗;中途斷網改用強制結束後重開加 Range 續傳測試驗證(見 Deviation)

### W-03 — 查詢與主畫面卡片(`lookup`)

- [x] 關鍵字 `ytsearch10:`、單一影片(`v=` 預設 `--no-playlist`)、播放清單 / 頻道(`-I 1:200`、`truncated`)
- [x] 主畫面：大輸入框、影片卡片(縮圖用 `i.ytimg.com`、CSP)、「整個清單」「全部存成音樂」
- [ ] Verify: 參數組裝與網址判斷單元測試;前端 Vitest(網址 / 關鍵字);手動用注音搜尋(gap:注音輸入沒辦法自動化，留給 W-09 的手動 QA;這次是用 SendKeys 直接送中文字)

Evidence: PR #4。`npm run rs:test` 36 passed,新增：
- `Input::parse`:關鍵字、`v=` / `list=` / youtu.be / shorts、沒有 scheme 的連結
- 所有 yt-dlp 呼叫都帶 `--js-runtimes deno:` 和 `--ffmpeg-location`
- 搜尋用 `ytsearch10:` operand(查詢字串開頭是 `-` 也不會被當成 flag)
- Mix 連結預設 `--no-playlist`;選「整個清單」時改成 `-I 1:200`,網址前加 `--`
- 解析搜尋結果時略過直播;播放清單超過上限標成 truncated

其他驗證:
- `-- --ignored`:真的搜尋「鄧麗君 月亮代表我的心」,1.6s 回傳卡片；真的查 `@YouTube` 頻道首頁，只回傳影片
- `npm test` 6 passed(含時長格式)
- 手動 debug exe:中文搜尋出現有縮圖、時長、頻道和兩顆按鈕的卡片(截圖);貼上 Mix 連結只列出一首，並出現「整個清單都要」(截圖);貼上「看這個頻道 https://www.youtube.com/@TED」列出 200 部影片並顯示截斷提示(截圖);不存在的 handle 顯示「找不到…請確認網址」,不是網路錯誤

對抗性 review 修正：上述 lookup 邊界情況、「整個清單」改用當初的網址、字級全部 ≥ 1rem、accent 加深到 8.9:1、卡片邊框 4.8:1、查詢中按鈕用 `aria-disabled`(不丟焦點)、常駐 live region、存檔按鈕在 W-04 之前 disabled

Deviation:網址 / 關鍵字判斷原本打算在前端用 Vitest 測 → 改由 Rust 的 `Input::parse` 判斷，前端只把文字送過去 → 只在一處判斷，不會有兩套規則 → 測試在 `ytdlp::input`。

### W-04 — 下載排隊與進度(`queue`、`ytdlp`)

- [x] 一次一個的 worker、`job-updated` 整個快照
- [x] 進度 JSON(兩條 stream 合併)、`processing` 不定進度
- [x] yt-dlp 一律帶 `--js-runtimes deno:<bin>\deno.exe`(W-02 留下的缺口)— 已在 W-03 用 `ytdlp::base_args` 做好,PR #4 的單元測試涵蓋
- [x] 唯一檔名、`-o`、`--no-overwrites`、`--print after_move:filepath`(`--trim-filenames` 改成自己截到 120 字，見 Deviation)
- [x] 影片格式選擇器 + ffprobe 檢查，非 H.264 才轉;音樂 mp3 + metadata;`cookies.txt`
- [x] 取消：結束 Job Object + 清暫存檔
- [x] Verify: 進度解析與檔名單元測試(真實輸出 fixture);`cargo test -- --ignored` 下載短 CC 影片音樂 / 影片各一次;手動取消後無殘檔

Evidence: PR #5。`npm run rs:test` 62 passed,新增：
- naming:Windows 不允許的字元、保留名稱、長標題、`(2)` 判斷不分大小寫、`%` 跳脫
- 進度：兩條 stream 合成一條、處理階段、estimate fallback、最終路徑
- 下載參數：音樂 / 影片 / cookies
- 錯誤代碼:format_unavailable、network
- queue(用假的 runner 測):依序一次一個、取消排隊中的不會執行、取消執行中的不影響下一個、失敗帶錯誤碼且不擋後面的
- 清檔只刪這個下載的檔案
- queue:同一首同格式不重複排入、批次略過已存過的、每個發出的 snapshot `rev` 都比前一個大
- 單一 stream 的影片進度能跑到 100%;檔名長度用 UTF-16 計算;onedir 資料夾的解壓、替換與 rollback,以及拒絕跳出資料夾的壓縮檔項目

`-- --ignored` 7 passed(含 W-02 / W-03 的整合測試):
- 真的下載「Me at the zoo」音樂 + 影片：輸出 mp3 和 mp4,影片是 h264,資料夾只剩這兩個檔
- 下載長片到一半取消：狀態 Canceled,資料夾是空的
- ffmpeg 產生的 VP9 會轉成 H.264,暫存檔已清掉
- 跑完取消測試後 `%TEMP%\_MEI*` 數量不變(onedir 版不再漏)

`npm test` 11 passed。手動用 debug exe 貼網址，按「存成音樂」:下載中顯示進度條、「快好了，正在處理…」和取消按鈕；完成後顯示「好了！已存到『下載 › YouTube』」和打開資料夾(兩張截圖)。測試產生的檔案已刪除。

Deviation:
- 檔名長度：原本用 `--trim-filenames 150` → 改由 `naming::sanitize` 自己截到 120 字 → 我們是給 yt-dlp 已經組好的檔名，自己截比較確定 → naming 單元測試。
- 同一首先存音樂再存影片時，影片會叫 `歌 (2).mp4` → 每個下載的檔名開頭必須獨一無二，取消時才能安全刪掉所有以它開頭的檔案 → runner 整合測試。
- 「打開資料夾」:原本前端直接呼叫 opener → 改成 Rust 的 `open_folder(job_id)` → 只能打開這個 app 產生的檔案，不開放任意路徑；檔案被搬走時改開它所在的資料夾。
- yt-dlp 版本：原本用 onefile `yt-dlp.exe`(W-02)→ 改成 onedir `yt-dlp_win.zip`,裝在 `bin\yt-dlp\` → review 實測每取消一次就在 `%TEMP%` 漏一個 24 MB 的 `_MEI*` → 修好後取消測試前後數量不變;design / CLAUDE.md 已同步。

對抗性 review 修正:onedir yt-dlp;批次與重複點擊不重複排入;`rev` 與只在狀態真的改變時才發 event;`list_jobs` 讓重新整理後能同步;焦點交給新出現的按鈕;live region 只播報狀態、不唸百分比;ffprobe 失敗時保留檔案；轉檔後換檔會重試；轉檔一律輸出 `.mp4`;已取消的工作不再啟動 yt-dlp;清暫存檔會重試;cookies 每次用暫存複本，查詢時也帶上;單一 stream 的進度修正;檔名用 UTF-16 長度計算。沒修：worker panic(release 是 `panic=abort`,panic 時整個 app 直接結束);長片轉檔沒有進度、「其他下載」不會收起來(兩項都寫進 vault backlog)。

Done: PR #5 · verify: `npm test`, `npm run rs:test` and `npm run rs:test -- --ignored` → pass · note: 真實下載音樂 / 影片;取消後資料夾為空由整合測試驗證，手動只驗了下載進度與完成畫面

### W-05 — 錯誤分類與自動修復

- [x] 錯誤代碼分類(design 表中全部代碼),前端白話文案
- [x] yt-dlp 版本比較(stable / nightly 格式),從 W-02 移過來
- [x] yt-dlp 更新：一天一次背景檢查、換檔前取 queue 鎖、`.new` → `.old` 換檔
- [ ] rollback 到上一版 (removed: 見 Deviation)
- [x] `extractor` 自動流程:stable → nightly → 「過一兩天再試」
- [x] `copy_diagnostics`、log 輪替、啟動時記錄工具版本與 hash
- [ ] 手動在 UI 按「複製問題資訊」(gap:下載層的失敗在 UI 很難故意製造，只有 build 和型別檢查，留給 W-09 的手動 QA)
- [x] Verify: 每個代碼都有真實 stderr fixture 的單元測試;換檔 / rollback 測試;手動放舊版 yt-dlp 觸發自動更新後重試

Evidence: PR #6。`npm run rs:test` 79 passed,新增：
- `ytdlp::errors`:真實 stderr 的分類
  - 私人影片、無法播放、格式不存在、年齡限制
  - 走 proxy 連不上(WinError 10061)
  - 舊版 yt-dlp 的「The page needs to be reloaded.」
  - 不存在的 handle(HTTP 404)
  - 從 issue 抄來的 bot check / 429 / Errno 28
- 版本比較
- 修復流程(用寫好劇本的假 update):不需要更新就不試；stable 修好就停；沒有新的 stable 就試 nightly;更新失敗時不重試下載
- 冷卻與暫停：同一個 channel 30 分鐘內不重查;403 / 429 後整體暫停一小時；每日檢查 24 小時內跳過
- 錯誤分類補上：清單 / 頻道不存在(NotFound)、本機存檔被擋(LocalIo,不觸發更新)、「content isn’t available, try again later」(BotCheck)
- log:UTC 時間格式、輪替最多留 5 個檔

`-- --ignored` 9 passed,新增：
- 用真的舊版 yt-dlp 2025.01.15 下載「Me at the zoo」:失敗 → 狀態變成 Updating → 自動更新到 stable → 重試成功；`state.json` 版本已更新
- 真的 stable 更新：第一次會裝，再查一次會說已是最新，每日檢查會跳過

`npm test` 11 passed。

缺口:「複製問題資訊」按鈕沒有在 UI 上實際按過(下載層的失敗在 UI 很難故意製造),只有 build 和型別檢查;留給 W-09 的手動 QA。

Deviation:
- `format_unavailable` 原本設計成**不**觸發更新 → 改成也會先試更新 → YouTube 改版常見的症狀之一就是「Requested format is not available」(只給 SABR 串流)→ `errors` 單元測試。
- 新增錯誤代碼 `NotFound`(HTTP 404、網址格式錯誤)→ 這類錯誤原本會落到 `extractor`,讓打錯的網址也去觸發 yt-dlp 更新。
- 「新版 yt-dlp 有 regression 就 rollback 到上一版」removed from scope → stable 很少出 regression,而且修復流程本來就會再試 nightly;再加 rollback 需要記住多個舊版本、判斷哪一版「比較好」,複雜度不划算 → 更新後驗證不過時的 rollback(`verify_or_rollback`)仍然保留。
- 「換檔前取 queue 鎖」實作成 `Updater` 的兩把鎖：
  - 讀寫鎖：跑 yt-dlp 時拿讀鎖；只在換資料夾和驗證那一下拿寫鎖，下載和解壓都在鎖外做
  - `files` 鎖：`prepare` 和 updater 共用，`bin\` 與 `state.json` 只有一個人在寫

對抗性 review 修正：
- 寫鎖範圍縮小
- 更新冷卻與限流暫停；驗證不過的版本不再重裝
- 失敗後版本已被換過時，直接用新版重試
- prepare 與 updater 共用 `files` 鎖
- 更新中可以取消
- 搜尋時也顯示「正在更新下載工具」
- 每 6 小時重跑一次每日檢查
- 全新安裝時記錄檢查時間
- log 輪替在檔案被鎖住時不會洗掉歷史
- 重試的新下載不沿用舊的「已複製」狀態；複製失敗會顯示提示
- 新增本機存檔被擋的文案

### W-06 — 轉檔(`convert`)

- [x] 拖檔 / 選檔、MP3 / MP4、輸出命名規則、不可寫時改存預設資料夾
- [x] `ffprobe` 總長度 + `-nostats -progress pipe:1`
- [x] Verify: 命名規則單元測試;wav → mp3、mkv → mp4(由 `-- --ignored` 整合測試驗證，不是手動)
- [ ] 手動用檔案對話框選檔、拖檔進視窗(gap:沒辦法自動化操作，留給 W-09 的手動 QA)

Evidence: PR #7。`npm run rs:test` 88 passed,新增：
- 輸出命名：換副檔名放在原檔旁邊；同格式加「(轉檔)」;撞名時加 `(2)`;原檔共用開頭時不會被跳號，原檔內容不變
- 結束前才出現的同名檔(含大小寫不同)不會被覆蓋，會改用 `(2)`
- 原檔資料夾不可寫時，改存到預設資料夾
- 取消時只刪自己的 `.ytf-part`
- ffmpeg 進度解析(含 `N/A`)、ffprobe JSON 解析
- 音樂只取第一條音訊並略過字幕、影片會縮放到偶數尺寸

`-- --ignored`:
- wav → `tone.mp3`
- 321x241、yuv444p 的 mkv → `odd.mp4`
- 沒有音軌的影片轉音樂會回 `no_audio`
- 過程中有回報進度，結束後沒有殘留的 `.ytf-part`

`npm test` 13 passed(含「改存預設資料夾」的文字判斷、另一種工作的排隊數)。debug exe 轉檔畫面截圖。

對抗性 review 修正:
- 奇數尺寸
- 寫到 `.ytf-part` 再用不覆蓋的方式放到最終檔名(修掉關閉 app 時留下截斷檔，以及 `-n` 遇到已存在的檔案仍 exit 0 被誤報成功)
- 不再寫可寫測試檔
- 只取第一條影像和第一條音訊，略過字幕
- 新錯誤代碼 `no_audio` / `is_folder`
- 改存預設資料夾時，完成文字寫明實際存到哪裡
- 另一種工作卡在前面時顯示排隊說明
- 切換畫面時把焦點交給新畫面
- 格式選項支援方向鍵、勾選圖示和高對比樣式
- 沒選檔就按開始時顯示提示
- 檔案類型補齊，並加上「所有檔案」
- 準備階段可以取消
- stderr 只留最後 40 行

缺口：檔案對話框和拖放沒辦法自動化操作，留給 W-09 的手動 QA。

Deviation:轉檔的唯一檔名原本打算沿用下載的「檔名開頭唯一」規則 → 改成只比對完整檔名，最終檔名在寫完後才用 hard link 不覆蓋地決定 → 原檔本身就和輸出共用開頭(`歌.wav` / `歌.mp3`),沿用的話會變成 `歌 (2).mp3`,而且取消時的「刪掉所有以這個開頭的檔案」會連原檔一起刪掉 → 轉檔失敗或取消時只刪它寫出的那一個檔，不走 `remove_job_files`。

### W-07 — 設定與細節

- [x] 設定 4 項(資料夾、字級 大 / 特大、亮 / 暗、中文 / English),損毀 fallback
- [ ] 舊資料夾 `下載\youtube-downloads` 提示(gap:實作完成，但這台電腦沒有舊資料夾，留給 W-09 在家人電腦上確認)
- [x] 下載中關閉視窗先確認
- [ ] Verify: 設定 fallback 單元測試;手動切特大字級與暗色;對比用 DevTools 檢查達 AAA(gap:對比是用色碼計算，沒有開 DevTools 實測)

Evidence: PR #8。`npm run rs:test` 96 passed,新增：
- settings:
  - 沒有檔案時用預設值(特大、亮、中文)
  - 損毀的檔案另存成 `settings.broken.json`
  - 看不懂的值只重置那一欄
  - 只送有改的欄位(patch),合併時不會動到其他欄位
  - 選的資料夾不見了改用預設資料夾
  - 可寫測試不會留下檔案
- queue:`cancel_all`

`-- --ignored` 10 passed,包含改用 `.ytf` 暫存名之後的真實下載。`npm test` 14 passed(i18n:中英切換、資料夾標籤)。

手動用 debug exe:
- 設定畫面截圖
- 暗色 + English + 大字，整個畫面都套用(截圖)
- 下載長片時關閉視窗，跳出確認，焦點在「繼續下載」(截圖)
- 按「確定關掉」:先取消、清理，再結束;「下載 › YouTube」沒有殘檔；下載途中的檔名是 `….ytf.mp3`

測試用的 `settings.json` 和下載檔都已刪除。

對比(用色碼計算)：
- 文字皆 ≥ 7:1,例如 `#f1f5f9` 對 `#0b1220` 約 17:1、`#06223f` 對 `#93c5fd` 約 10:1
- 進度條：暗色改成深色軌道後，與 `#93c5fd` 約 8:1
- 分段按鈕的焦點框改畫在按鈕外側

對抗性 review 修正：
- 暗色進度條看不見
- 「確定關掉」後的確認視窗：兩顆按鈕都不能按，並顯示「正在停止」
- 設定改成只送有改的欄位，在 Rust 端依序合併，快速連點不會互蓋
- 前端沒回應時，第二次關閉直接放行，不會關不掉
- 確認視窗開著時背景設成 `inert`,關閉後焦點回到原位
- 不能寫入的資料夾會被擋下(`folder_not_writable`)
- 設定相關 command 改成 async,慢的磁碟不會凍住視窗
- 讀設定失敗時用預設值，不白屏
- 視窗標題跟著語言切換
- 「打開看看」失敗時會顯示提示
- 存設定遇到鎖檔會重試
- 分段按鈕支援方向鍵
- **下載也改成先寫 `<base>.ytf.<ext>`,完成後才不覆蓋地放到最終檔名**:關機、登出或強制結束時，不會留下名字看起來完整的截斷檔

沒修：暗色啟動時會先閃一下亮色(要等設定讀回來)。

Deviation:
- 完成文字原本固定寫「下載 › YouTube」→ 改成依實際輸出路徑顯示資料夾 → 可以自選資料夾後，固定文字會說錯地方 → jobs 測試。
- 「確定關掉」原本直接關 → 改成先 `cancel_all` 並等清理(最多 10 秒)→ 實測直接被 Job Object 殺掉時，會留下名字看起來完整、其實是截斷的 `.mp3`。之後再加上 `.ytf` 暫存名，從根本避免。
- 舊資料夾提示原本寫「提示一次」→ 改成每次啟動都顯示，直到按「知道了」→ 只顯示一次的話，家人當下沒注意就再也找不到。

## Phase 2 — 發布與上線

Checkpoint: 家人電腦裝的是 GitHub Release 的 2.0.0,之後的版本會自動更新;舊 repo 已封存。

### W-08 — 發布與自動更新

- [x] updater 金鑰：私鑰與密碼進 Actions secret(私鑰從檔案直接設，不經過對話)
- [ ] 私鑰檔與密碼另外離線備份 (removed: 移到 W-09,家人安裝 2.0.0 前完成;見 Deviation)
- [x] release workflow:打 tag → NSIS 安裝檔 + `latest.json`,正式、標為 latest;`installMode: "passive"`,只在啟動且佇列空時安裝
- [x] app 圖示(原稿 `src-tauri/icons/app-icon.svg`)
- [x] Verify: 發 2.0.0-rc.1 → rc.2,已裝的 rc.1 啟動後自動更新到 rc.2;安裝檔 ≤ 15 MB

Evidence:
- release run 38017280242(v2.0.0-rc.1)與 38017703978(v2.0.0-rc.2)都成功：正式、非 prerelease,`releases/latest` 指向 rc.2;assets 有 `*-setup.exe`、`.sig`、`latest.json`
- 安裝檔 4.4 MB(workflow 的大小檢查步驟)
- 這台電腦用 `/S` 安裝 rc.1(currentUser,裝在 `%LOCALAPPDATA%\youtube-to-your-format`),打開後 log:`installing app update 2.0.0-rc.1 -> 2.0.0-rc.2` → `app update downloaded, starting the installer` → 2 秒後 `starting youtube-to-your-format 2.0.0-rc.2`;exe 與解除安裝登錄的版本都變成 2.0.0-rc.2,`bin\` 的工具與設定保留
- `npm test` 16 passed、`npm run lint`、`npm run rs:clippy` 通過;debug exe 啟動正常(updater plugin 讀得懂設定)

對抗性 review 修正：
- updater builder 的 timeout 算整個請求，慢速網路下載不完 → 改成檢查 20 秒、下載 10 分鐘，各自用 tokio timeout
- 更新結束後才到的 progress event 不再把畫面切回「正在更新」
- 找不到安裝檔時大小檢查會失敗，不再靜默通過

Deviation:
- 金鑰原本在對話框用 `--ci -p` 產生 → 密碼出現在對話紀錄 → 改在使用者自己的 PowerShell 互動輸入，重新產生金鑰與密碼(還沒簽過任何東西，換掉沒有成本)
- 金鑰離線備份原本是 W-08 的步驟 → 移到 W-09 家人安裝前 → 2026-10-10 Bobo 還沒有這台電腦以外的備份，而真正不能再換金鑰的時間點是家人裝了 2.0.0 之後
- 發現：安裝位置就是 app 的資料夾(`%LOCALAPPDATA%\youtube-to-your-format`)。解除安裝只刪自己裝的檔、`RMDir` 不遞迴，所以工具與設定不會被刪;但「刪除 app 資料」勾選框刪的是 `%LOCALAPPDATA%\<identifier>`,對我們沒作用，解除安裝後會留下約 120 MB 工具 → 寫進 vault backlog,不在 W-08 處理

Done: PR #10 · verify: manual: 安裝 v2.0.0-rc.1 後打開，自己更新並重新打開成 v2.0.0-rc.2;安裝檔 4.4 MB → pass · verify: `npm test` and `npm run rs:clippy` → pass · note: 金鑰離線備份移到 W-09 家人安裝前

### W-09 — 上線到家人電腦

- [ ] **家人安裝前**:updater 私鑰檔與密碼備份到這台電腦以外(密碼管理器、隨身碟或雲端，兩者不放同一處),`CLAUDE.md` 改成實際的備份方式
- [ ] 在沒有管理員權限的帳號跑完 `CLAUDE.md` 手動 QA 清單
- [ ] 補驗前面留下的手動缺口，驗完回到該項目寫 `Done:`:注音搜尋(W-03)、按「複製問題資訊」(W-05)、檔案對話框與拖檔(W-06)、舊資料夾提示與 DevTools 對比(W-07)
- [ ] 家人電腦：解除安裝舊版、安裝 2.0.0、確認舊資料夾提示
- [ ] 舊 repo README 指向新 repo 並封存;`side-project-ideas.md` 同步
- [ ] Verify: QA 清單逐項結果寫進 `docs/rust-rewrite/evidence/qa-2.0.0.md`

## Former IDs

| Old | New |
|---|---|
| W01 | W-01 |
| W02 | W-02 |
| W03 | W-03 |
| W04 | W-04 |
| W05 | W-05 |
| W06 | W-06 |
| W07 | W-07 |
| W08 | W-08 |
| W09 | W-09 |

## Close checklist

- [ ] Every checked item has evidence; open items are ticked, removed from scope, or the work stays open.
- [ ] Content that still describes the running system is promoted to reference docs, README or CLAUDE.md.
- [ ] `state: done`, then the folder moves to `docs/archive/`.
