---
state: in-progress
updated: 2026-11-13
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

Done: PR #2 · verify: `npm test` and `npm run rs:test` → pass · note: CI run 37511365158 pass;debug exe 開出視窗

### W-02 — 子行程與工具安裝(`process.rs`、`tools`)

Done: PR #3 · verify: `npm test`, `npm run rs:test` and `npm run rs:test -- --ignored` → pass · note: 手動刪 bin 後重新準備、全程無 console 視窗;中途斷網改用強制結束後重開加 Range 續傳測試驗證(見 [evidence/w-02.md](evidence/w-02.md))

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

Done: PR #10 · verify: manual: 安裝 v2.0.0-rc.1 後打開，自己更新並重新打開成 v2.0.0-rc.2;安裝檔 4.4 MB → pass · verify: `npm test` and `npm run rs:clippy` → pass · note: 金鑰離線備份移到 W-09 家人安裝前

### W-10 — 解除安裝可以清掉工具與設定

Done: PR #11 · verify: manual: rc.2 自動更新到 rc.3 後工具保留;靜默解除安裝保留;勾「刪除 app 資料」解除安裝後只剩使用者自己的檔 → pass · note: 工具實際約 324 MB,不是先前估的 120 MB

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
