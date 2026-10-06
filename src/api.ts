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
