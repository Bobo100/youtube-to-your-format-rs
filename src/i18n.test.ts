import { afterEach, describe, expect, it } from "vitest";
import { dirLabel, errorMessage, folderLabel, setLanguage, t } from "./i18n";

afterEach(() => setLanguage("zh"));

describe("i18n", () => {
  it("defaults to Traditional Chinese and switches to English", () => {
    expect(t("appTitle")).toBe("YouTube 下載");
    setLanguage("en");
    expect(t("appTitle")).toBe("YouTube Downloader");
    expect(t("playlistCount", { count: 3 })).toBe("3 songs");
    expect(errorMessage("network")).toContain("internet");
  });

  it("labels folders in plain words", () => {
    expect(dirLabel("C:/Users/a/Downloads/YouTube")).toBe("下載 › YouTube");
    expect(folderLabel("E:/音樂/老歌/a.mp3")).toBe("音樂 › 老歌");
    setLanguage("en");
    expect(dirLabel("C:/Users/a/Downloads/YouTube")).toBe("Downloads › YouTube");
  });
});
