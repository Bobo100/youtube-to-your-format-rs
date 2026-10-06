import { describe, expect, it } from "vitest";
import { formatDuration } from "./format";

describe("formatDuration", () => {
  it("formats minutes and hours", () => {
    expect(formatDuration(205)).toBe("3:25");
    expect(formatDuration(59)).toBe("0:59");
    expect(formatDuration(3725)).toBe("1:02:05");
    expect(formatDuration(null)).toBeNull();
  });
});
