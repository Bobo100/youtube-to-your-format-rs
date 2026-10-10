import { describe, expect, it } from "vitest";
import { preparingView, updatingView } from "./preparing";
import { errorMessage } from "./i18n";

const base = { tool: "deno" as const, phase: "download" as const };

describe("preparingView", () => {
  it("spreads progress across steps", () => {
    expect(preparingView({ ...base, step: 2, steps: 3, received: 50, total: 100 }).percent).toBe(50);
    expect(preparingView({ ...base, step: 3, steps: 3, received: 100, total: 100 }).percent).toBe(100);
  });

  it("is indeterminate before the first event, while extracting, and without a total", () => {
    expect(preparingView(null).percent).toBeNull();
    expect(preparingView({ ...base, phase: "extract", step: 1, steps: 1, received: 0, total: null }).percent).toBeNull();
    expect(preparingView({ ...base, step: 1, steps: 2, received: 10, total: null }).percent).toBe(0);
  });

  it("names the step in plain Chinese", () => {
    expect(preparingView({ ...base, step: 1, steps: 3, received: 0, total: 1 }).detail).toBe("第 1 步，共 3 步");
  });
});

describe("updatingView", () => {
  it("shows download progress, capped at 100", () => {
    expect(updatingView({ version: "2.0.1", received: 25, total: 100 }).percent).toBe(25);
    expect(updatingView({ version: "2.0.1", received: 120, total: 100 }).percent).toBe(100);
  });

  it("is indeterminate until the size is known", () => {
    expect(updatingView({ version: "2.0.1", received: 0, total: null }).percent).toBeNull();
  });
});

describe("errorMessage", () => {
  it("falls back to the generic message for unknown codes", () => {
    expect(errorMessage("tool_blocked")).toBe("下載工具被防毒軟體擋住了");
    expect(errorMessage("something_new")).toBe("準備失敗了");
  });
});
