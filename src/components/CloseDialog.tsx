import { useEffect, useRef, useState } from "react";
import { closePromptShown } from "../api";
import { t } from "../i18n";

type Props = {
  onKeep: () => void;
  onClose: () => Promise<void>;
};

/** Shown when the window is closed while downloads are still running. The rest
 * of the app is made `inert` by the parent while this is open. */
export function CloseDialog({ onKeep, onClose }: Props) {
  const keep = useRef<HTMLButtonElement>(null);
  const [closing, setClosing] = useState(false);

  useEffect(() => {
    keep.current?.focus();
    // Tells Rust the question is on screen; without this a second close goes through.
    closePromptShown().catch(() => undefined);
  }, []);

  const close = () => {
    if (closing) return;
    setClosing(true);
    onClose().catch(() => setClosing(false));
  };

  return (
    <div className="overlay">
      <div
        className="dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="close-title"
        aria-describedby="close-body"
        onKeyDown={(event) => event.key === "Escape" && !closing && onKeep()}
      >
        <h2 id="close-title" className="big">
          {t("closeTitle")}
        </h2>
        <p id="close-body" aria-live="polite">
          {closing ? t("stopping") : t("closeBody")}
        </p>
        <div className="acts">
          {/* The safe choice is first and focused. */}
          <button ref={keep} className="btn" aria-disabled={closing} onClick={() => !closing && onKeep()}>
            {t("keepDownloading")}
          </button>
          <button className="btn sec" aria-disabled={closing} onClick={close}>
            {t("closeAnyway")}
          </button>
        </div>
      </div>
    </div>
  );
}
