import { useEffect, useRef } from "react";
import { t } from "../i18n";

type Props = {
  onKeep: () => void;
  onClose: () => void;
};

/** Shown when the window is closed while downloads are still running. */
export function CloseDialog({ onKeep, onClose }: Props) {
  const keep = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    keep.current?.focus();
  }, []);
  return (
    <div className="overlay">
      <div
        className="dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="close-title"
        aria-describedby="close-body"
        onKeyDown={(event) => event.key === "Escape" && onKeep()}
      >
        <h2 id="close-title" className="big">
          {t("closeTitle")}
        </h2>
        <p id="close-body">{t("closeBody")}</p>
        <div className="acts">
          {/* The safe choice is first and focused. */}
          <button ref={keep} className="btn" onClick={onKeep}>
            {t("keepDownloading")}
          </button>
          <button className="btn sec" onClick={onClose}>
            {t("closeAnyway")}
          </button>
        </div>
      </div>
    </div>
  );
}
