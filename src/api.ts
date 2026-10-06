import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type ToolsProgress = {
  step: number;
  steps: number;
  tool: "ytdlp" | "ffmpeg" | "deno";
  phase: "download" | "extract";
  received: number;
  total: number | null;
};

/** Rejects with an error code string such as "network" or "tool_blocked". */
export function prepareTools(): Promise<void> {
  return invoke("prepare_tools");
}

export function onToolsProgress(handler: (progress: ToolsProgress) => void): Promise<UnlistenFn> {
  return listen<ToolsProgress>("tools-progress", (event) => handler(event.payload));
}

export type VideoCard = {
  id: string;
  url: string;
  title: string;
  channel: string | null;
  durationS: number | null;
  thumbnail: string;
};

export type Lookup = {
  kind: "video" | "playlist" | "search";
  title: string | null;
  items: VideoCard[];
  truncated: boolean;
  skipped: number;
  hasPlaylist: boolean;
};

export type SaveFormat = "audio" | "video";

/** Rejects with an error code string such as "network" or "lookup_failed". */
export function lookup(input: string, wholePlaylist = false): Promise<Lookup> {
  return invoke("lookup", { input, wholePlaylist });
}

export type JobState = "queued" | "downloading" | "processing" | "updating" | "done" | "failed" | "canceled";

export type Job = {
  id: number;
  /** Higher is newer; events may arrive out of order. */
  rev: number;
  videoId: string;
  url: string;
  title: string;
  format: SaveFormat;
  state: JobState;
  progress: number | null;
  outputPath: string | null;
  error: string | null;
};

/** `skipDone`: leave out items already saved (used by "全部存成音樂"). */
export function enqueue(items: VideoCard[], format: SaveFormat, skipDone = false): Promise<number[]> {
  return invoke("enqueue", {
    items: items.map(({ id, url, title }) => ({ id, url, title })),
    format,
    skipDone,
  });
}

export function listJobs(): Promise<Job[]> {
  return invoke("list_jobs");
}

export function cancelJob(id: number): Promise<void> {
  return invoke("cancel_job", { id });
}

export function openFolder(id: number): Promise<void> {
  return invoke("open_folder", { id });
}

export function onJobUpdated(handler: (job: Job) => void): Promise<UnlistenFn> {
  return listen<Job>("job-updated", (event) => handler(event.payload));
}

/** Plain-text report for Bobo: versions, what failed, recent log. */
export function diagnostics(id?: number): Promise<string> {
  return invoke("diagnostics", { id: id ?? null });
}
