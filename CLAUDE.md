# youtube-to-your-format 2.0

給**眼睛不好、不太會用電腦的長輩**用的 YouTube 下載 / 轉檔 Windows 桌面程式。

技術:Tauri 2(Rust 後端)、前端 Vite + React + TypeScript。下載交給 yt-dlp,轉檔交給 ffmpeg;JS runtime 用 Deno。

這是舊 Electron 版 [SideProject-Youtube-To-Your-Format](https://github.com/Bobo100/SideProject-Youtube-To-Your-Format) 的重寫。

## 架構心智模型

- **前端**:`src/`,打包進 app 的網頁，由 WebView2 顯示
- **後端**:同一個程式裡的 Rust 程式碼，不是伺服器。分兩層:
  - `src-tauri/core/`(crate `ytf-core`):所有邏輯，不依賴 Tauri,單元測試都寫在這裡
  - `src-tauri/src/`:只有 Tauri 接線(`commands.rs`、`lib.rs`),不放邏輯、不寫測試
- **前後端唯一介面**:`commands.rs` 的 Tauri commands,以及 `job-updated` / `tools-progress` 兩個 event。契約見 [design.md](docs/rust-rewrite/design.md#前後端契約commandsrs--events)
- **三個外部工具**:yt-dlp、ffmpeg、Deno,都不放進安裝檔。第一次開啟時下載到 `%LOCALAPPDATA%\youtube-to-your-format\bin\`
- **對外連線**:零伺服器，只讀 GitHub Releases 與 YouTube

## 鐵則

- **所有子行程只能經過 `process.rs` 啟動**:它負責 `CREATE_NO_WINDOW`(不跳 console 視窗)、Job Object(取消或當掉時整組結束)、UTF-8。直接用 `std::process::Command` 會讓長輩看到黑色視窗
- **畫面上不出現英文錯誤或技術詞**:錯誤在 Rust 端分類成代碼，文案由前端 i18n 決定；原文只進「複製問題資訊」與 log
- **一個畫面一個主要動作**:字級 18 / 20px、按鈕高 ≥ 52px、文字對比 AAA、圖示都配文字、不用 emoji。新畫面先對照 [mockup](docs/mockups/ui/README.md)
- **下載的工具一律驗 SHA-256**:ffmpeg / Deno 的 hash 寫死在程式裡;yt-dlp 比對同 release 的 `SHA2-256SUMS`
- **測試寫在 `ytf-core`,不寫在 Tauri crate**:連結 Tauri 的測試執行檔沒有 app manifest,一跑就 `STATUS_ENTRYPOINT_NOT_FOUND`,所以 Tauri crate 設了 `test = false`
- **yt-dlp 用 onedir 版(`bin\yt-dlp\`),不要換回 onefile `yt-dlp.exe`**:onefile 每次執行都解壓到 `%TEMP%\_MEI*`,被 Job Object 殺掉時不會清掉,每次取消漏 24 MB
- **寫檔一律先寫暫存名，完成後才用 `naming::place_without_replacing` 放到最終檔名**:下載寫 `<base>.ytf.<ext>`、轉檔寫 `.ytf-part`。被殺掉(關機、強制結束)時不會留下名字看起來完整的截斷檔，也不會覆蓋使用者的檔案
- **每個下載的檔名開頭(`<base>.`)必須獨一無二**(`naming::unique_base`):取消時會刪掉資料夾裡所有以它開頭的檔案。所以同一首先存音樂再存影片，影片會叫 `歌 (2).mp4`。**轉檔不適用這條**:原檔和輸出共用開頭，轉檔只能刪它寫出的那一個檔(`convert::output_path`)
- **app 資料夾裡新增或改名檔案時，同步改 `src-tauri/windows/hooks.nsh`**:解除安裝勾「刪除 app 資料」時只刪那裡列出的名字(Tauri 內建的勾選框只刪 `%LOCALAPPDATA%\<identifier>`,不是我們的資料夾)
- **升級 ffmpeg / Deno = 改 `core/src/tools/manifest.rs` 的常數**:URL、版本、SHA-256(對照發布者自己的 checksum)。yt-dlp 不固定版本

## 指令

```bash
npm install
npm start                          # 開發(tauri dev)
npm run lint && npm test           # 前端
npm run rs:clippy && npm run rs:test
npm run rs:build-debug             # 不打包的 debug exe(內嵌前端,不需 dev server)
```

**Toolchain 坑**:Bobo 的電腦沒有 MSVC build tools,Git Bash 的 `/usr/bin/link` 還會蓋掉 `link.exe`,所以直接跑 `cargo build` 會在 link 失敗。本機的 Rust 指令一律走 `scripts/with-gnu-toolchain.ps1`(WinLibs + `stable-x86_64-pc-windows-gnu`),上面的 `rs:*` 與 `start` 已包好。CI 與 release 在 GitHub Actions 上用 MSVC,正式安裝檔不受影響。要在本機做 GNU 安裝檔時，先看 bai-e-desktop-pet 的 `verify-gnu-runtime.ps1`(GNU 版 NSIS 可能漏包 `WebView2Loader.dll`)。

## 發版

1. `src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`package.json` 的 version 改成同一個(`Cargo.lock`、`package-lock.json` 裡本 app 那一筆跟著改)
2. merge 進 main 後打 tag `v<version>` 並 push:`.github/workflows/release.yml` 產出 NSIS 安裝檔、`.sig` 與 `latest.json`,發布成**正式、非 prerelease** 的 release(已安裝的 app 讀 `releases/latest/download/latest.json`,draft 或 prerelease 都不會被當成 latest)
3. workflow 最後一步檢查安裝檔 ≤ 15 MB

**自動更新只在啟動時、佇列還沒開始前做**(`app_update.rs`):Windows 的安裝程式會關掉 app。檢查最多 20 秒、下載最多 10 分鐘，失敗就照用目前版本;不要把 timeout 設在 updater builder 上(它算整個請求，慢速網路永遠下載不完)。debug build 不檢查更新。

**updater 金鑰**:私鑰在 Actions secret `TAURI_SIGNING_PRIVATE_KEY`(密碼 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`),公鑰寫在 `tauri.conf.json`。**私鑰檔與密碼要另外離線備份**(rust-rewrite/W-09 家人安裝前完成):私鑰檔原檔在 Bobo 的 A 電腦 `%USERPROFILE%\.tauri\ytf-updater.key`。兩者遺失任一個，已安裝的 app 就再也收不到更新(只能請家人手動重裝)。家人裝了之後不要換公鑰:舊版只認這把。

**圖示**:原稿 `src-tauri/icons/app-icon.svg`,改完跑 `npx tauri icon src-tauri/icons/app-icon.svg -o src-tauri/icons`,再刪掉產生的 `android/`、`ios/`、`64x64.png`(只出 Windows)。

## 文件地圖

| 內容 | 位置 |
|---|---|
| 設計(進行中) | [docs/rust-rewrite/design.md](docs/rust-rewrite/design.md) |
| 執行 plan(唯一的 checklist) | [docs/rust-rewrite/plan.md](docs/rust-rewrite/plan.md) |
| UI mockup | [docs/mockups/ui/](docs/mockups/ui/README.md) |
| 決策經過、backlog | vault `_memory/youtube-to-your-format-rs/` |

## 手動 QA 清單(發版前，在沒有管理員權限的 Windows 帳號上跑)

1. 安裝
2. 第一次準備：中途斷網再恢復，確認能續傳
3. 全程沒有出現 console 視窗
4. 用注音輸入搜尋
5. 特大字級
6. 下載含 `list=` 的網址：預設只下一首
7. 下載播放清單
8. 下載中取消：資料夾裡沒有殘檔
9. 下載中關閉視窗：會先問
10. 放舊版 yt-dlp 模擬 YouTube 改版，確認會自動更新後重試
11. app 自我更新
12. 解除安裝舊版 Electron app,確認會出現舊資料夾提示

## Commit 格式

`type(scope): 中文描述`,type 用 `feat`、`fix`、`docs`、`refactor`、`chore`、`test`、`perf`、`ci`。每個改動都走 branch → PR。
