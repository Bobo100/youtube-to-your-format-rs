import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { convertFiles, type Job, type SaveFormat } from "../api";
import { JobStatus } from "../components/JobStatus";
import { t } from "../i18n";
import { FileIcon, MusicIcon, VideoIcon } from "../icons";

type Props = {
  jobs: Job[];
};

const MEDIA_EXTENSIONS = ["mp3", "mp4", "m4a", "wav", "wma", "aac", "flac", "ogg", "opus", "webm", "mkv", "mov", "avi", "wmv", "3gp"];

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function Convert({ jobs }: Props) {
  const [files, setFiles] = useState<string[]>([]);
  const [format, setFormat] = useState<SaveFormat>("audio");
  const [dragging, setDragging] = useState(false);
  const [starting, setStarting] = useState(false);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "over" || event.payload.type === "enter") setDragging(true);
        else if (event.payload.type === "leave") setDragging(false);
        else if (event.payload.type === "drop") {
          setDragging(false);
          setFiles(event.payload.paths);
        }
      })
      .then((unlisten) => {
        if (cancelled) unlisten();
        else stop = unlisten;
      });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  const pick = async () => {
    const picked = await open({
      multiple: true,
      directory: false,
      filters: [{ name: t("mediaFiles"), extensions: MEDIA_EXTENSIONS }],
    });
    if (picked) setFiles(Array.isArray(picked) ? picked : [picked]);
  };

  const start = () => {
    if (files.length === 0 || starting) return;
    setStarting(true);
    convertFiles(files, format)
      .then(() => setFiles([]))
      .catch((err: unknown) => console.error("convert failed", err))
      .finally(() => setStarting(false));
  };

  const convertJobs = jobs.filter((job) => job.kind === "convert").reverse();

  return (
    <>
      <button className={dragging ? "drop over" : "drop"} onClick={pick}>
        <FileIcon />
        <span className="big">{files.length > 0 ? t("filesChosen", { count: files.length }) : t("dropHere")}</span>
        <span className="help">{files.length > 0 ? files.map(fileName).join("、") : t("orPick")}</span>
      </button>

      <p className="lbl" id="convert-to">
        {t("convertTo")}
      </p>
      <div className="chips" role="radiogroup" aria-labelledby="convert-to">
        <button role="radio" aria-checked={format === "audio"} className={format === "audio" ? "chip on" : "chip"} onClick={() => setFormat("audio")}>
          <MusicIcon />
          {t("formatAudio")}
        </button>
        <button role="radio" aria-checked={format === "video"} className={format === "video" ? "chip on" : "chip"} onClick={() => setFormat("video")}>
          <VideoIcon />
          {t("formatVideo")}
        </button>
      </div>

      <button className="btn wide" aria-disabled={files.length === 0 || starting} onClick={start}>
        {t("startConvert")}
      </button>
      <p className="help">{t("convertWhere")}</p>

      {convertJobs.length > 0 && (
        <section className="res" aria-label={t("convertList")}>
          <h2 className="status">{t("convertList")}</h2>
          {convertJobs.map((job) => (
            <article key={job.id} className="item compact">
              <div className="info">
                <h3 className="ttl">{job.title}</h3>
                <JobStatus key={job.id} job={job} onRetry={() => convertFiles([job.url], job.format)} />
              </div>
            </article>
          ))}
        </section>
      )}
    </>
  );
}
