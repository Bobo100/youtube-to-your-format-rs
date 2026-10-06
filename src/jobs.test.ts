import { describe, expect, it } from "vitest";
import type { Job } from "./api";
import { jobStatusText, latestJobFor, upsertJob } from "./jobs";

const job = (over: Partial<Job>): Job => ({
  id: 1,
  videoId: "abc",
  url: "u",
  title: "t",
  format: "audio",
  state: "queued",
  progress: null,
  outputPath: null,
  error: null,
  ...over,
});

describe("jobs", () => {
  it("replaces snapshots by id and appends new ones", () => {
    const jobs = upsertJob(upsertJob([], job({ id: 1 })), job({ id: 2 }));
    const updated = upsertJob(jobs, job({ id: 1, state: "done" }));
    expect(updated.map((j) => [j.id, j.state])).toEqual([
      [1, "done"],
      [2, "queued"],
    ]);
  });

  it("finds the newest job for a card", () => {
    const jobs = [job({ id: 1 }), job({ id: 3 }), job({ id: 2, videoId: "other" })];
    expect(latestJobFor(jobs, "abc")?.id).toBe(3);
    expect(latestJobFor(jobs, "none")).toBeUndefined();
  });

  it("says what is happening in plain words", () => {
    expect(jobStatusText(job({ state: "downloading", progress: 0.637 }))).toBe("正在下載音樂… 64%");
    expect(jobStatusText(job({ state: "downloading", format: "video", progress: 0 }))).toBe("正在下載影片… 0%");
    expect(jobStatusText(job({ state: "processing" }))).toBe("快好了，正在處理…");
  });
});
