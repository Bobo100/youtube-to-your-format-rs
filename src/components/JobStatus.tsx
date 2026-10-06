import { useEffect, useRef, useState } from "react";
import { cancelJob, diagnostics, openFolder, type Job } from "../api";
import { errorMessage, t } from "../i18n";
import { FolderIcon } from "../icons";
import { isActive, jobStateText, jobStatusText } from "../jobs";

type Props = {
  job: Job;
  onRetry: () => void;
};

export function JobStatus({ job, onRetry }: Props) {
  const measurable = job.state === "downloading" && job.progress !== null;
  const primary = useRef<HTMLButtonElement>(null);
  const [openError, setOpenError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  // The button that was pressed disappears when the state changes (save → cancel →
  // open folder), which drops keyboard focus to <body>. Hand it to the new button.
  useEffect(() => {
    const lost = !document.activeElement || document.activeElement === document.body;
    if (lost) primary.current?.focus();
  }, [job.state]);

  const copyDiagnostics = () => {
    diagnostics(job.id)
      .then((text) => navigator.clipboard.writeText(text))
      .then(() => setCopied(true))
      .catch((err: unknown) => console.error("copy failed", err));
  };

  const open = () => {
    setOpenError(null);
    openFolder(job.id).catch((code: unknown) => setOpenError(String(code)));
  };

  return (
    <div className="job">
      {/* Announce state changes only; the percentage would be re-read on every update. */}
      <p className="visually-hidden" aria-live="polite">
        {jobStateText(job)}
      </p>
      {isActive(job) && (
        <div
          className={measurable ? "bar" : "bar indeterminate"}
          role="progressbar"
          aria-label={jobStateText(job)}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={measurable ? Math.round((job.progress ?? 0) * 100) : undefined}
        >
          <i style={measurable ? { width: `${(job.progress ?? 0) * 100}%` } : undefined} />
        </div>
      )}
      <p className={job.state === "done" ? "status ok" : "status"} aria-hidden="true">
        {jobStatusText(job)}
      </p>
      {job.state === "failed" && <p className="warn">{errorMessage(job.error ?? "", "error.download_failed")}</p>}
      {openError && <p className="warn">{errorMessage(openError, "error.open_failed")}</p>}
      <div className="acts">
        {isActive(job) && (
          <button ref={primary} className="btn sec" onClick={() => cancelJob(job.id)}>
            {t("cancel")}
          </button>
        )}
        {job.state === "done" && (
          <button ref={primary} className="btn" onClick={open}>
            <FolderIcon />
            {t("openFolder")}
          </button>
        )}
        {job.state === "failed" && (
          <button ref={primary} className="btn" onClick={onRetry}>
            {t("retry")}
          </button>
        )}
        {job.state === "failed" && (
          <button className="btn sec" onClick={copyDiagnostics}>
            {t("copyDiagnostics")}
          </button>
        )}
      </div>
      {copied && (
        <p className="status ok" role="status">
          {t("copied")}
        </p>
      )}
    </div>
  );
}
