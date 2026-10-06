import type { SaveFormat, VideoCard } from "../api";
import { formatDuration } from "../format";
import { t } from "../i18n";
import { MusicIcon, VideoIcon } from "../icons";

type Props = {
  card: VideoCard;
  onSave: (card: VideoCard, format: SaveFormat) => void;
};

export function VideoCardView({ card, onSave }: Props) {
  const duration = formatDuration(card.durationS);
  return (
    <article className="item">
      <div className="thumb">
        <img src={card.thumbnail} alt="" loading="lazy" />
        {duration && <span>{duration}</span>}
      </div>
      <div className="info">
        <h3 className="ttl">{card.title}</h3>
        {card.channel && <p className="meta">{card.channel}</p>}
        <div className="acts">
          <button className="btn" onClick={() => onSave(card, "audio")}>
            <MusicIcon />
            {t("saveAudio")}
          </button>
          <button className="btn sec" onClick={() => onSave(card, "video")}>
            <VideoIcon />
            {t("saveVideo")}
          </button>
        </div>
      </div>
    </article>
  );
}
