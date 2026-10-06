import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  closeAnyway,
  enqueue,
  getSettings,
  listJobs,
  onJobUpdated,
  onToolsProgress,
  openOldFolder,
  prepareTools,
  setSettings,
  type AppSettings,
  type Job,
  type SaveFormat,
  type SettingsView,
  type ToolsProgress,
  type VideoCard,
} from "./api";
import { CloseDialog } from "./components/CloseDialog";
import { upsertJob } from "./jobs";
import { dirLabel, setLanguage, t } from "./i18n";
import { BackIcon, ConvertIcon, GearIcon } from "./icons";
import { Convert } from "./screens/Convert";
import { Home } from "./screens/Home";
import { Preparing } from "./screens/Preparing";
import { Settings } from "./screens/Settings";
import "./styles.css";

type Phase = "preparing" | "ready";
type Screen = "home" | "convert" | "settings";

/** Applies text size, colours and language to the whole document. */
function applySettings(settings: AppSettings) {
  const root = document.documentElement;
  root.style.fontSize = settings.fontSize === "large" ? "18px" : "20px";
  root.dataset.theme = settings.theme;
  root.lang = settings.language === "en" ? "en" : "zh-Hant-TW";
  setLanguage(settings.language);
}

const FIRST_CONTROL: Record<Screen, string> = { home: "what", convert: "drop-zone", settings: "back" };

export default function App() {
  const [phase, setPhase] = useState<Phase>("preparing");
  const [progress, setProgress] = useState<ToolsProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [screen, setScreen] = useState<Screen>("home");
  const [view, setView] = useState<SettingsView | null>(null);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const [confirmClose, setConfirmClose] = useState(false);

  useEffect(() => {
    getSettings().then((loaded) => {
      applySettings(loaded.settings);
      setView(loaded);
    });
  }, []);

  const changeSettings = (settings: AppSettings) => {
    setSettingsError(null);
    setSettings(settings)
      .then((saved) => {
        applySettings(saved.settings);
        setView(saved);
      })
      .catch((code: unknown) => setSettingsError(String(code)));
  };

  // The pressed header button unmounts on a screen change; give focus to the
  // new screen's first control instead of dropping it to <body>.
  useEffect(() => {
    if (phase !== "ready") return;
    document.getElementById(FIRST_CONTROL[screen])?.focus();
  }, [screen, phase]);

  useEffect(() => {
    let stops: (() => void)[] = [];
    let cancelled = false;
    Promise.all([
      onJobUpdated((job) => setJobs((current) => upsertJob(current, job))),
      listen("confirm-close", () => setConfirmClose(true)),
    ]).then((unlisteners) => {
      if (cancelled) {
        unlisteners.forEach((stop) => stop());
        return;
      }
      stops = unlisteners;
      // After a reload the queue may already hold jobs; rev keeps the newest copy.
      listJobs().then((existing) => setJobs((current) => existing.reduce(upsertJob, current)));
    });
    return () => {
      cancelled = true;
      stops.forEach((stop) => stop());
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

  const dismissOldFolder = () => view && changeSettings({ ...view.settings, oldFolderHintDismissed: true });

  if (!view) return null;

  return (
    <div className="app">
      <header className="top">
        {screen === "home" ? (
          <span>{t("appTitle")}</span>
        ) : (
          <button id="back" className="tb" onClick={() => setScreen("home")}>
            <BackIcon />
            {t("back")}
          </button>
        )}
        {phase === "ready" && screen === "home" && (
          <span className="tools">
            <button className="tb" onClick={() => setScreen("convert")}>
              <ConvertIcon />
              {t("convert")}
            </button>
            <button className="tb" onClick={() => setScreen("settings")}>
              <GearIcon />
              {t("settings")}
            </button>
          </span>
        )}
        {screen !== "home" && <span>{t(screen === "convert" ? "convert" : "settings")}</span>}
      </header>
      <main className="body">
        {phase === "preparing" ? (
          <Preparing progress={progress} error={error} onRetry={retry} />
        ) : (
          <>
            {/* Home stays mounted so its search results survive a visit elsewhere. */}
            <div hidden={screen !== "home"}>
              {view.oldFolder && (
                <div className="notice" role="status">
                  <p>{t("oldFolderHint", { folder: dirLabel(view.oldFolder) })}</p>
                  <div className="acts">
                    <button className="btn sec" onClick={() => openOldFolder().catch(() => undefined)}>
                      {t("openOldFolder")}
                    </button>
                    <button className="btn sec" onClick={dismissOldFolder}>
                      {t("dismiss")}
                    </button>
                  </div>
                </div>
              )}
              <Home jobs={jobs} onSave={save} />
            </div>
            {screen === "convert" && <Convert jobs={jobs} />}
            {screen === "settings" && <Settings view={view} error={settingsError} onChange={changeSettings} />}
          </>
        )}
      </main>
      {confirmClose && <CloseDialog onKeep={() => setConfirmClose(false)} onClose={() => closeAnyway()} />}
    </div>
  );
}
