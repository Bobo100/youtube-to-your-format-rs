import type { AppUpdateProgress, ToolsProgress } from "./api";
import { t } from "./i18n";

export type PreparingView = {
  /** 0–100, or null while the current step has no measurable progress. */
  percent: number | null;
  detail: string;
};

export function preparingView(progress: ToolsProgress | null): PreparingView {
  if (!progress) return { percent: null, detail: t("preparingHint") };
  if (progress.phase === "extract") {
    return { percent: null, detail: t("preparingExtract", { step: progress.step, steps: progress.steps }) };
  }
  const stepShare = 100 / progress.steps;
  const within = progress.total ? Math.min(progress.received / progress.total, 1) : 0;
  return {
    percent: Math.round((progress.step - 1 + within) * stepShare),
    detail: t("preparingStep", { step: progress.step, steps: progress.steps }),
  };
}

export function updatingView(update: AppUpdateProgress): PreparingView {
  const percent = update.total ? Math.min(Math.round((update.received / update.total) * 100), 100) : null;
  return { percent, detail: t("updatingHint") };
}
