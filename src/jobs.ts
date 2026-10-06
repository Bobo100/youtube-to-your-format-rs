import type { Job } from "./api";
import { t } from "./i18n";

/** Replaces a job by id unless we already hold a newer snapshot of it. */
export function upsertJob(jobs: Job[], job: Job): Job[] {
  const index = jobs.findIndex((j) => j.id === job.id);
  if (index === -1) return [...jobs, job].sort((a, b) => a.id - b.id);
  if (jobs[index].rev > job.rev) return jobs;
  return jobs.map((j, i) => (i === index ? job : j));
}

/** The newest job started from this card, if any. */
export function latestJobFor(jobs: Job[], videoId: string): Job | undefined {
  return jobs.reduce<Job | undefined>((latest, j) => (j.videoId === videoId && (!latest || j.id > latest.id) ? j : latest), undefined);
}

export function isActive(job: Job): boolean {
  return job.state === "queued" || job.state === "downloading" || job.state === "processing" || job.state === "updating";
}

/** Jobs that no card on screen is showing (a card shows only its newest job). */
export function offScreenJobs(jobs: Job[], onScreenVideoIds: string[]): Job[] {
  const shown = new Set(onScreenVideoIds.map((id) => latestJobFor(jobs, id)?.id));
  return jobs.filter((job) => !shown.has(job.id)).reverse();
}

/** Short state name for screen readers: announced on change, without the percentage. */
export function jobStateText(job: Job): string {
  return job.state === "downloading" ? t("jobDownloadingShort", { what: job.format === "audio" ? t("whatAudio") : t("whatVideo") }) : jobStatusText(job);
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
    case "updating":
      return t("jobUpdating");
    case "done":
      return t("jobDone");
    case "canceled":
      return t("jobCanceled");
    case "failed":
      return t("jobFailed");
  }
}
