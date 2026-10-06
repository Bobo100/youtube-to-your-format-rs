import { useCallback, useEffect, useState } from "react";
import { onToolsProgress, prepareTools, type ToolsProgress } from "./api";
import { t } from "./i18n";
import { Preparing } from "./screens/Preparing";
import "./styles.css";

type Phase = "preparing" | "ready";

export default function App() {
  const [phase, setPhase] = useState<Phase>("preparing");
  const [progress, setProgress] = useState<ToolsProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

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
    const unlisten = onToolsProgress(setProgress);
    prepare();
    return () => {
      unlisten.then((stop) => stop());
    };
  }, [prepare]);

  return (
    <div className="app">
      <header className="top">{t("appTitle")}</header>
      <main className="body">
        {phase === "preparing" ? (
          <Preparing progress={progress} error={error} onRetry={retry} />
        ) : (
          <p className="big">{t("readyPlaceholder")}</p>
        )}
      </main>
    </div>
  );
}
