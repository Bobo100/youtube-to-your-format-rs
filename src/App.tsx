import { useCallback, useEffect, useState } from "react";
import {
  enqueue,
  listJobs,
  onJobUpdated,
  onToolsProgress,
  prepareTools,
  type Job,
  type SaveFormat,
  type ToolsProgress,
  type VideoCard,
} from "./api";
import { upsertJob } from "./jobs";
import { t } from "./i18n";
import { BackIcon, ConvertIcon } from "./icons";
import { Convert } from "./screens/Convert";
import { Home } from "./screens/Home";
import { Preparing } from "./screens/Preparing";
import "./styles.css";

type Phase = "preparing" | "ready";
type Screen = "home" | "convert";

export default function App() {
  const [phase, setPhase] = useState<Phase>("preparing");
  const [progress, setProgress] = useState<ToolsProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [screen, setScreen] = useState<Screen>("home");

  // The pressed header button unmounts on a screen change; give focus to the
  // new screen's first control instead of dropping it to <body>.
  useEffect(() => {
    if (phase !== "ready") return;
    document.getElementById(screen === "home" ? "what" : "drop-zone")?.focus();
  }, [screen, phase]);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let cancelled = false;
    onJobUpdated((job) => setJobs((current) => upsertJob(current, job))).then((unlisten) => {
      if (cancelled) {
        unlisten();
        return;
      }
      stop = unlisten;
      // After a reload the queue may already hold jobs; rev keeps the newest copy.
      listJobs().then((existing) => setJobs((current) => existing.reduce(upsertJob, current)));
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  const save = (cards: VideoCard[], format: SaveFormat, skipDone = false) =>
    enqueue(cards, format, skipDone)
      .then(() => undefined)
      .catch((err: unknown) => console.error("enqueue failed", err));

  const prepare = useCallback(() => {
    prepareTools()
      .then(() => setPhase("ready"))
      .catch((code: unknown) => setError(String(code)));
  }, []);

  const retry = () => {
    setError(null);
    setProgress(null);
    prepare();
  };

  useEffect(() => {
    // Start only after the listener is registered so no early progress is missed.
    // In StrictMode the first effect is cleaned up before listen() resolves, so
    // prepare() runs once.
    let cancelled = false;
    let stop: (() => void) | undefined;
    onToolsProgress(setProgress).then((unlisten) => {
      if (cancelled) {
        unlisten();
        return;
      }
      stop = unlisten;
      prepare();
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [prepare]);

  return (
    <div className="app">
      <header className="top">
        {screen === "convert" ? (
          <button className="tb" onClick={() => setScreen("home")}>
            <BackIcon />
            {t("back")}
          </button>
        ) : (
          <span>{t("appTitle")}</span>
        )}
        {phase === "ready" && screen === "home" && (
          <button className="tb" onClick={() => setScreen("convert")}>
            <ConvertIcon />
            {t("convert")}
          </button>
        )}
        {screen === "convert" && <span>{t("convert")}</span>}
      </header>
      <main className="body">
        {phase === "preparing" ? (
          <Preparing progress={progress} error={error} onRetry={retry} />
        ) : (
          <>
            {/* Home stays mounted so its search results survive a visit to 轉檔. */}
            <div hidden={screen !== "home"}>
              <Home jobs={jobs} onSave={save} />
            </div>
            {screen === "convert" && <Convert jobs={jobs} />}
          </>
        )}
      </main>
    </div>
  );
}
