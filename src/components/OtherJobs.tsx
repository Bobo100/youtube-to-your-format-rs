import type { Job } from "../api";
import { t } from "../i18n";
import { JobStatus } from "./JobStatus";

type Props = {
  jobs: Job[];
  onRetry: (job: Job) => void;
};

/** Downloads whose card is no longer on screen (e.g. after a new search). */
export function OtherJobs({ jobs, onRetry }: Props) {
  if (jobs.length === 0) return null;
  return (
    <section className="res" aria-label={t("otherJobs")}>
      <h2 className="status">{t("otherJobs")}</h2>
      {jobs.map((job) => (
        <article key={job.id} className="item compact">
          <div className="info">
            <h3 className="ttl">{job.title}</h3>
            <JobStatus job={job} onRetry={() => onRetry(job)} />
          </div>
        </article>
      ))}
    </section>
  );
}
