import { cancelJob, openFolder, type Job } from "../api";
import { errorMessage, t } from "../i18n";
import { FolderIcon } from "../icons";
import { isActive, jobStatusText } from "../jobs";

type Props = {
  job: Job;
  onRetry: () => void;
};

export function JobStatus({ job, onRetry }: Props) {
  const measurable = job.state === "downloading" && job.progress !== null;
  return (
    <div className="job" aria-live="polite">
      {isActive(job) && (
        <div
          className={measurable ? "bar" : "bar indeterminate"}
          role="progressbar"
          aria-label={jobStatusText(job)}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={measurable ? Math.round((job.progress ?? 0) * 100) : undefined}
        >
          <i style={measurable ? { width: `${(job.progress ?? 0) * 100}%` } : undefined} />
        </div>
      )}
      <p className={job.state === "done" ? "status ok" : "status"}>{jobStatusText(job)}</p>
      {job.state === "failed" && <p className="warn">{errorMessage(job.error ?? "", "error.download_failed")}</p>}
      <div className="acts">
        {isActive(job) && (
          <button className="btn sec" onClick={() => cancelJob(job.id)}>
            {t("cancel")}
          </button>
        )}
        {job.state === "done" && (
          <button className="btn" onClick={() => openFolder(job.id)}>
            <FolderIcon />
            {t("openFolder")}
          </button>
        )}
        {job.state === "failed" && (
          <button className="btn" onClick={onRetry}>
            {t("retry")}
          </button>
        )}
      </div>
    </div>
  );
}
