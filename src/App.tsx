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
import { Home } from "./screens/Home";
import { Preparing } from "./screens/Preparing";
import "./styles.css";

type Phase = "preparing" | "ready";

export default function App() {
  const [phase, setPhase] = useState<Phase>("preparing");
  const [progress, setProgress] = useState<ToolsProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [jobs, setJobs] = useState<Job[]>([]);

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
      <header className="top">{t("appTitle")}</header>
      <main className="body">
        {phase === "preparing" ? (
          <Preparing progress={progress} error={error} onRetry={retry} />
        ) : (
          <Home jobs={jobs} onSave={save} />
        )}
      </main>
    </div>
  );
}
