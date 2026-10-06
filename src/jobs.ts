import type { Job } from "./api";
import { t } from "./i18n";

/** Replaces a job by id; events carry the full snapshot. */
export function upsertJob(jobs: Job[], job: Job): Job[] {
  const index = jobs.findIndex((j) => j.id === job.id);
  return index === -1 ? [...jobs, job] : jobs.map((j, i) => (i === index ? job : j));
}

/** The newest job started from this card, if any. */
export function latestJobFor(jobs: Job[], videoId: string): Job | undefined {
  return jobs.reduce<Job | undefined>((latest, j) => (j.videoId === videoId && (!latest || j.id > latest.id) ? j : latest), undefined);
}

export function isActive(job: Job): boolean {
  return job.state === "queued" || job.state === "downloading" || job.state === "processing";
}

export function jobStatusText(job: Job): string {
  const what = job.format === "audio" ? t("whatAudio") : t("whatVideo");
  switch (job.state) {
    case "queued":
      return t("jobQueued");
    case "downloading":
      return t("jobDownloading", { what, percent: Math.round((job.progress ?? 0) * 100) });
    case "processing":
      return t("jobProcessing");
    case "done":
      return t("jobDone");
    case "canceled":
      return t("jobCanceled");
    case "failed":
      return t("jobFailed");
  }
}
