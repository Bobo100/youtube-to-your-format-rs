import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { convertFiles, type Job, type SaveFormat } from "../api";
import { JobStatus } from "../components/JobStatus";
import { errorMessage, t } from "../i18n";
import { CheckIcon, FileIcon, MusicIcon, VideoIcon } from "../icons";
import { activeOfKind } from "../jobs";

type Props = {
  jobs: Job[];
};

const MEDIA_EXTENSIONS = [
  "mp3", "mp4", "m4a", "m4v", "wav", "wma", "aac", "aiff", "amr", "flac", "ogg", "opus", "webm", "mkv",
  "mov", "avi", "wmv", "flv", "3gp", "mts", "m2ts", "ts", "mpg", "mpeg", "vob",
];

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function Convert({ jobs }: Props) {
  const [files, setFiles] = useState<string[]>([]);
  const [format, setFormat] = useState<SaveFormat>("audio");
  const [dragging, setDragging] = useState(false);
  const [starting, setStarting] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const audioChip = useRef<HTMLButtonElement>(null);
  const videoChip = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "over" || event.payload.type === "enter") setDragging(true);
        else if (event.payload.type === "leave") setDragging(false);
        else if (event.payload.type === "drop") {
          setDragging(false);
          setMessage(null);
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

  const pick = () => {
    open({
      multiple: true,
      directory: false,
      filters: [
        { name: t("mediaFiles"), extensions: MEDIA_EXTENSIONS },
        { name: t("allFiles"), extensions: ["*"] },
      ],
    })
      .then((picked) => {
        if (!picked) return;
        setMessage(null);
        setFiles(Array.isArray(picked) ? picked : [picked]);
      })
      .catch((err: unknown) => console.error("file dialog failed", err));
  };

  const start = () => {
    if (starting) return;
    if (files.length === 0) {
      setMessage(t("pickFirst"));
      return;
    }
    setStarting(true);
    convertFiles(files, format)
      .then(() => {
        setFiles([]);
        setMessage(null);
      })
      .catch((err: unknown) => {
        console.error("convert failed", err);
        setMessage(errorMessage("convert_start_failed"));
      })
      .finally(() => setStarting(false));
  };

  // Radio group keyboard pattern: arrows move and select.
  const onChipKey = (event: KeyboardEvent) => {
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return;
    event.preventDefault();
    const next = format === "audio" ? "video" : "audio";
    setFormat(next);
    (next === "audio" ? audioChip : videoChip).current?.focus();
  };

  const convertJobs = jobs.filter((job) => job.kind === "convert").reverse();
  const waitingDownloads = activeOfKind(jobs, "download");

  const chip = (value: SaveFormat, label: string, icon: React.ReactNode, ref: React.RefObject<HTMLButtonElement | null>) => (
    <button
      ref={ref}
      role="radio"
      aria-checked={format === value}
      tabIndex={format === value ? 0 : -1}
      className={format === value ? "chip on" : "chip"}
      onClick={() => setFormat(value)}
      onKeyDown={onChipKey}
    >
      {format === value ? <CheckIcon /> : icon}
      {label}
    </button>
  );

  return (
    <>
      <button id="drop-zone" className={dragging ? "drop over" : "drop"} onClick={pick}>
        <FileIcon />
        <span className="big">{files.length > 0 ? t("filesChosen", { count: files.length }) : t("dropHere")}</span>
        <span className="help">{files.length > 0 ? files.map(fileName).join("、") : t("orPick")}</span>
      </button>

      <p className="lbl" id="convert-to">
        {t("convertTo")}
      </p>
      <div className="chips" role="radiogroup" aria-labelledby="convert-to">
        {chip("audio", t("formatAudio"), <MusicIcon />, audioChip)}
        {chip("video", t("formatVideo"), <VideoIcon />, videoChip)}
      </div>

      <button className="btn wide" aria-disabled={files.length === 0 || starting} onClick={start}>
        {t("startConvert")}
      </button>
      {message && (
        <p className="warn" role="alert">
          {message}
        </p>
      )}
      <p className="help">{t("convertWhere")}</p>
      {waitingDownloads > 0 && <p className="help">{t("waitingDownloads", { count: waitingDownloads })}</p>}

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
