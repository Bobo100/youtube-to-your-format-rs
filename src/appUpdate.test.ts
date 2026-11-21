import { describe, expect, it } from "vitest";
import { checkEnabled, reduceUpdate, type UpdateState } from "./appUpdate";

const idle: UpdateState = { kind: "idle" };

describe("manual update check", () => {
  it("walks idle → checking → upToDate / available", () => {
    const checking = reduceUpdate(idle, { type: "check" });
    expect(checking).toEqual({ kind: "checking" });
    expect(reduceUpdate(checking, { type: "checked", version: null })).toEqual({ kind: "upToDate" });
    expect(reduceUpdate(checking, { type: "checked", version: "2.0.0-rc.5" })).toEqual({
      kind: "available",
      version: "2.0.0-rc.5",
    });
  });

  it("installs only after a new version was found, and reports percent", () => {
    expect(reduceUpdate(idle, { type: "install" })).toBe(idle);
    const available: UpdateState = { kind: "available", version: "2.0.0-rc.5" };
    const installing = reduceUpdate(available, { type: "install" });
    expect(installing).toEqual({ kind: "installing", version: "2.0.0-rc.5", percent: null });
    expect(reduceUpdate(installing, { type: "progress", received: 5, total: 20 })).toEqual({
      kind: "installing",
      version: "2.0.0-rc.5",
      percent: 25,
    });
    expect(reduceUpdate(installing, { type: "progress", received: 30, total: 20 })).toMatchObject({ percent: 100 });
    expect(reduceUpdate(installing, { type: "progress", received: 5, total: null })).toMatchObject({ percent: null });
  });

  it("ignores progress outside an install and double checks", () => {
    expect(reduceUpdate(idle, { type: "progress", received: 1, total: 2 })).toBe(idle);
    const checking: UpdateState = { kind: "checking" };
    expect(reduceUpdate(checking, { type: "check" })).toBe(checking);
  });

  it("keeps the error code on failure and allows another try", () => {
    const failed = reduceUpdate({ kind: "checking" }, { type: "failed", code: "network" });
    expect(failed).toEqual({ kind: "failed", code: "network" });
    expect(reduceUpdate(failed, { type: "check" })).toEqual({ kind: "checking" });
  });

  it("disables the check while busy, checking or installing", () => {
    expect(checkEnabled(idle, false)).toBe(true);
    expect(checkEnabled({ kind: "upToDate" }, false)).toBe(true);
    expect(checkEnabled({ kind: "failed", code: "network" }, false)).toBe(true);
    expect(checkEnabled(idle, true)).toBe(false);
    expect(checkEnabled({ kind: "checking" }, false)).toBe(false);
    expect(checkEnabled({ kind: "installing", version: "x", percent: null }, false)).toBe(false);
  });
});
