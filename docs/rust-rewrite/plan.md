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

- [ ] 一次一個的 worker、`job-updated` 整個快照
- [ ] 進度 JSON(兩條 stream 合併)、`processing` 不定進度
- [x] yt-dlp 一律帶 `--js-runtimes deno:<bin>\deno.exe`(W02 留下的缺口)— 已在 W03 用 `ytdlp::base_args` 做好,PR #4 的單元測試涵蓋
- [ ] 唯一檔名、`-o`、`--no-overwrites --trim-filenames`、`--print after_move:filepath`
- [ ] 影片格式選擇器 + ffprobe 檢查，非 H.264 才轉;音樂 mp3 + metadata;`cookies.txt`
- [ ] 取消：結束 Job Object + 清暫存檔
- [ ] Verify: 進度解析與檔名單元測試(真實輸出 fixture);`cargo test -- --ignored` 下載短 CC 影片音樂 / 影片各一次;手動取消後無殘檔

## W05 — 錯誤分類與自動修復

- [ ] 錯誤代碼分類(design 表中全部代碼),前端白話文案
- [ ] yt-dlp 版本比較(stable / nightly 格式),從 W02 移過來
- [ ] yt-dlp 更新：一天一次背景檢查、換檔前取 queue 鎖、`.new` → `.old` 換檔、rollback
- [ ] `extractor` 自動流程:stable → nightly → 「過一兩天再試」
- [ ] `copy_diagnostics`、log 輪替、啟動時記錄工具版本與 hash
- [ ] Verify: 每個代碼都有真實 stderr fixture 的單元測試;換檔 / rollback 測試;手動放舊版 yt-dlp 觸發自動更新後重試

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
