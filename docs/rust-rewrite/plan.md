---
state: in-progress
updated: 2026-10-07
---

# Rust 重寫 — Plan

**Goal:** 長輩在自己的 Windows 電腦上打歌名或貼網址就能存成音樂 / 影片;YouTube 常見的改版與 app 更新都自動處理,Bobo 不用再到現場重裝。

**Scope:** [design.md](design.md) 的全部範圍。2.0.0 裝到家人電腦後，舊 Electron repo 封存。

**Design:** [design.md](design.md)、mockup [docs/mockups/ui/](../mockups/ui/README.md)

每個 W 一個 PR(`type(scope): 中文描述`),PR merge 前在 branch 上勾選並附證據。

## W01 — 專案骨架與 CI

- [x] GitHub repo `Bobo100/youtube-to-your-format-rs`(public,同舊 repo)
- [x] Tauri 2 + Vite + React + TypeScript 骨架,app 名稱、identifier、視窗大小;NSIS currentUser 安裝
- [x] `CLAUDE.md`(架構心智模型、鐵則：子行程只走 `process.rs`、doc map、commit 格式、手動 QA 清單)+ `AGENTS.md` 指向 `CLAUDE.md`
- [x] Vitest、ESLint、`cargo test` / `clippy`;GitHub Actions Windows runner 在 PR 上跑
- [x] Verify: CI 綠燈;`npm run tauri dev` 開出空視窗

Evidence: PR #2。本機 `npm run lint`、`npm test`(1 passed)、`npm run build`、`npm run rs:clippy`、`npm run rs:test` 通過;CI run 37511365158 check pass(6m21s);debug exe 啟動後視窗標題「YouTube 下載」、中文正常(截圖)。
Deviation: 驗證視窗原本用 `npm run tauri dev` → 改用 `npm run rs:build-debug` 啟動內嵌前端的 debug exe → 不必另開 dev server → 同上截圖。另外本機沒有 MSVC build tools,Rust 指令改走 GNU toolchain wrapper(沿用 bai-e-desktop-pet),lib 只留 `rlib`(windows-gnu 下 cdylib 超過 ld 的 export ordinal 上限);CI / release 仍用 MSVC。

## W02 — 子行程與工具安裝(`process.rs`、`tools`)

- [x] `process.rs`:`CREATE_NO_WINDOW`、Job Object + `KILL_ON_JOB_CLOSE`、UTF-8、區分「不見了 / 被拒」
- [x] ffmpeg:選定固定版本、確認含 ffprobe(removed from scope: 重新上傳到 `tools-ffmpeg-<版本>` → 改直接用 GyanD,見 Deviation)
- [x] Deno:選定版本並確認符合 yt-dlp 最低需求
- [x] 三個工具的下載(續傳、重試)、SHA-256 驗證、解壓到 `%LOCALAPPDATA%\youtube-to-your-formatin\`
- [x] `--js-runtimes` 指向 Deno — W03 PR #4 用 `ytdlp::base_args` 做好
- [x] 第一次準備畫面 + `tools-progress`
- [x] Verify: 單元測試(SHA2-256SUMS 解析、驗證；版本比較移到 W05);手動：刪掉 `bin\` 後重開會重新準備，中途斷網可續傳;全程無 console 視窗

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
- 版本比較:原本放在 W02 → 移到 W05 → 只有 yt-dlp 更新會用到。
- 「中途斷網」:原本手動拔網路測 → 改用強制結束 app 再重開，加上 Range 續傳的整合測試 → 兩者都會留下 `.part`,走的是同一條路徑。
- 架構:原本把 `process.rs`、`tools/` 放在 `src-tauri/src/` → 改放進新的 workspace crate `ytf-core`,Tauri crate 設 `test = false` → 連結 Tauri 的測試執行檔一跑就 `STATUS_ENTRYPOINT_NOT_FOUND` → design / CLAUDE.md 已同步更新。

## W03 — 查詢與主畫面卡片(`lookup`)

- [x] 關鍵字 `ytsearch10:`、單一影片(`v=` 預設 `--no-playlist`)、播放清單 / 頻道(`-I 1:200`、`truncated`)
- [x] 主畫面：大輸入框、影片卡片(縮圖用 `i.ytimg.com`、CSP)、「整個清單」「全部存成音樂」
- [ ] Verify: 參數組裝與網址判斷單元測試;前端 Vitest(網址 / 關鍵字);手動用注音搜尋(gap:注音輸入沒辦法自動化，留給 W09 的手動 QA;這次是用 SendKeys 直接送中文字)

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

對抗性 review 修正：上述 lookup 邊界情況、「整個清單」改用當初的網址、字級全部 ≥ 1rem、accent 加深到 8.9:1、卡片邊框 4.8:1、查詢中按鈕用 `aria-disabled`(不丟焦點)、常駐 live region、存檔按鈕在 W04 之前 disabled

Deviation:網址 / 關鍵字判斷原本打算在前端用 Vitest 測 → 改由 Rust 的 `Input::parse` 判斷，前端只把文字送過去 → 只在一處判斷，不會有兩套規則 → 測試在 `ytdlp::input`。

## W04 — 下載排隊與進度(`queue`、`ytdlp`)

- [x] 一次一個的 worker、`job-updated` 整個快照
- [x] 進度 JSON(兩條 stream 合併)、`processing` 不定進度
- [x] yt-dlp 一律帶 `--js-runtimes deno:<bin>\deno.exe`(W02 留下的缺口)— 已在 W03 用 `ytdlp::base_args` 做好,PR #4 的單元測試涵蓋
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

`-- --ignored` 7 passed(含 W02 / W03 的整合測試):
- 真的下載「Me at the zoo」音樂 + 影片：輸出 mp3 和 mp4,影片是 h264,資料夾只剩這兩個檔
- 下載長片到一半取消：狀態 Canceled,資料夾是空的
- ffmpeg 產生的 VP9 會轉成 H.264,暫存檔已清掉
- 跑完取消測試後 `%TEMP%\_MEI*` 數量不變(onedir 版不再漏)

`npm test` 11 passed。手動用 debug exe 貼網址，按「存成音樂」:下載中顯示進度條、「快好了，正在處理…」和取消按鈕；完成後顯示「好了！已存到『下載 › YouTube』」和打開資料夾(兩張截圖)。測試產生的檔案已刪除。

Deviation:
- 檔名長度：原本用 `--trim-filenames 150` → 改由 `naming::sanitize` 自己截到 120 字 → 我們是給 yt-dlp 已經組好的檔名，自己截比較確定 → naming 單元測試。
- 同一首先存音樂再存影片時，影片會叫 `歌 (2).mp4` → 每個下載的檔名開頭必須獨一無二，取消時才能安全刪掉所有以它開頭的檔案 → runner 整合測試。
- 「打開資料夾」:原本前端直接呼叫 opener → 改成 Rust 的 `open_folder(job_id)` → 只能打開這個 app 產生的檔案，不開放任意路徑；檔案被搬走時改開它所在的資料夾。
- yt-dlp 版本：原本用 onefile `yt-dlp.exe`(W02)→ 改成 onedir `yt-dlp_win.zip`,裝在 `bin\yt-dlp\` → review 實測每取消一次就在 `%TEMP%` 漏一個 24 MB 的 `_MEI*` → 修好後取消測試前後數量不變;design / CLAUDE.md 已同步。

對抗性 review 修正:onedir yt-dlp;批次與重複點擊不重複排入;`rev` 與只在狀態真的改變時才發 event;`list_jobs` 讓重新整理後能同步;焦點交給新出現的按鈕;live region 只播報狀態、不唸百分比;ffprobe 失敗時保留檔案；轉檔後換檔會重試；轉檔一律輸出 `.mp4`;已取消的工作不再啟動 yt-dlp;清暫存檔會重試;cookies 每次用暫存複本，查詢時也帶上;單一 stream 的進度修正;檔名用 UTF-16 長度計算。沒修：worker panic(release 是 `panic=abort`,panic 時整個 app 直接結束);長片轉檔沒有進度、「其他下載」不會收起來(兩項都寫進 vault backlog)。

## W05 — 錯誤分類與自動修復

- [x] 錯誤代碼分類(design 表中全部代碼),前端白話文案
- [x] yt-dlp 版本比較(stable / nightly 格式),從 W02 移過來
- [x] yt-dlp 更新：一天一次背景檢查、換檔前取 queue 鎖、`.new` → `.old` 換檔
- [ ] rollback 到上一版(removed from scope: 見 Deviation)
- [x] `extractor` 自動流程:stable → nightly → 「過一兩天再試」
- [x] `copy_diagnostics`、log 輪替、啟動時記錄工具版本與 hash
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
- log:UTC 時間格式、輪替最多留 5 個檔

`-- --ignored` 9 passed,新增：
- 用真的舊版 yt-dlp 2025.01.15 下載「Me at the zoo」:失敗 → 狀態變成 Updating → 自動更新到 stable → 重試成功；`state.json` 版本已更新
- 真的 stable 更新：第一次會裝，再查一次會說已是最新，每日檢查會跳過

`npm test` 11 passed。

缺口:「複製問題資訊」按鈕沒有在 UI 上實際按過(下載層的失敗在 UI 很難故意製造),只有 build 和型別檢查;留給 W09 的手動 QA。

Deviation:
- `format_unavailable` 原本設計成**不**觸發更新 → 改成也會先試更新 → YouTube 改版常見的症狀之一就是「Requested format is not available」(只給 SABR 串流)→ `errors` 單元測試。
- 新增錯誤代碼 `NotFound`(HTTP 404、網址格式錯誤)→ 這類錯誤原本會落到 `extractor`,讓打錯的網址也去觸發 yt-dlp 更新。
- 「新版 yt-dlp 有 regression 就 rollback 到上一版」removed from scope → stable 很少出 regression,而且修復流程本來就會再試 nightly;再加 rollback 需要記住多個舊版本、判斷哪一版「比較好」,複雜度不划算 → 更新後驗證不過時的 rollback(`verify_or_rollback`)仍然保留。
- 「換檔前取 queue 鎖」實作成 `Updater` 的讀寫鎖：跑 yt-dlp 時拿讀鎖，換版本時拿寫鎖 → 查詢和下載都會被擋到換完為止。

## W06 — 轉檔(`convert`)

- [ ] 拖檔 / 選檔、MP3 / MP4、輸出命名規則、不可寫時改存預設資料夾
- [ ] `ffprobe` 總長度 + `-nostats -progress pipe:1`
- [ ] Verify: 命名規則單元測試;手動轉一個 wav → mp3、mp4 → mp4

## W07 — 設定與細節

- [ ] 設定 4 項(資料夾、字級 大 / 特大、亮 / 暗、中文 / English),損毀 fallback
- [ ] 舊資料夾 `下載\youtube-downloads` 提示、下載中關閉視窗先確認
- [ ] Verify: 設定 fallback 單元測試;手動切特大字級與暗色;對比用 DevTools 檢查達 AAA

## W08 — 發布與自動更新

- [ ] updater 金鑰：私鑰進 Actions secret,另外離線備份(位置寫進 `CLAUDE.md`)
- [ ] release workflow:打 tag → NSIS 安裝檔 + `latest.json`,正式、標為 latest;`installMode: "passive"`,只在啟動且佇列空時安裝
- [ ] Verify: 發 2.0.0-rc.1 → rc.2,已裝的 rc.1 啟動後自動更新到 rc.2;安裝檔 ≤ 15 MB

## W09 — 上線到家人電腦

- [ ] 在沒有管理員權限的帳號跑完 `CLAUDE.md` 手動 QA 清單
- [ ] 家人電腦：解除安裝舊版、安裝 2.0.0、確認舊資料夾提示
- [ ] 舊 repo README 指向新 repo 並封存;`side-project-ideas.md` 同步
- [ ] Verify: QA 清單逐項結果寫進 `docs/rust-rewrite/evidence/qa-2.0.0.md`

## Close checklist

- [ ] Every checked item has evidence; open items are ticked, removed from scope, or the work stays open.
- [ ] Content that still describes the running system is promoted to reference docs, README or CLAUDE.md.
- [ ] `state: done`, then the folder moves to `docs/archive/`.
