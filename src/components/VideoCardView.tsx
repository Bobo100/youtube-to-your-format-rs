import type { Job, SaveFormat, VideoCard } from "../api";
import { formatDuration } from "../format";
import { t } from "../i18n";
import { MusicIcon, VideoIcon } from "../icons";
import { isActive } from "../jobs";
import { JobStatus } from "./JobStatus";

type Props = {
  card: VideoCard;
  job?: Job;
  onSave: (card: VideoCard, format: SaveFormat) => void;
};

export function VideoCardView({ card, job, onSave }: Props) {
  const duration = formatDuration(card.durationS);
  const showSaveButtons = !job || job.state === "canceled" || job.state === "done";
  return (
    <article className="item">
      <div className="thumb">
        <img src={card.thumbnail} alt="" loading="lazy" />
        {duration && <span>{duration}</span>}
      </div>
      <div className="info">
        <h3 className="ttl">{card.title}</h3>
        {card.channel && <p className="meta">{card.channel}</p>}
        {job && <JobStatus job={job} onRetry={() => onSave(card, job.format)} />}
        {showSaveButtons && !(job && isActive(job)) && (
          <div className="acts">
            <button className={job?.state === "done" ? "btn sec" : "btn"} onClick={() => onSave(card, "audio")}>
              <MusicIcon />
              {t("saveAudio")}
            </button>
            <button className="btn sec" onClick={() => onSave(card, "video")}>
              <VideoIcon />
              {t("saveVideo")}
            </button>
          </div>
        )}
      </div>
    </article>
  );
}
