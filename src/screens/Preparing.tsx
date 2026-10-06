import { preparingView } from "../preparing";
import type { ToolsProgress } from "../api";
import { errorMessage, t } from "../i18n";

type Props = {
  progress: ToolsProgress | null;
  error: string | null;
  onRetry: () => void;
};

export function Preparing({ progress, error, onRetry }: Props) {
  if (error) {
    return (
      <section className="center" role="alert">
        <p className="big">{errorMessage(error)}</p>
        <button className="btn" onClick={onRetry}>
          {t("retry")}
        </button>
      </section>
    );
  }

  const view = preparingView(progress);
  return (
    <section className="center" aria-live="polite">
      <p className="big">{t("preparingTitle")}</p>
      <div
        className={view.percent === null ? "bar indeterminate" : "bar"}
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={view.percent ?? undefined}
      >
        <i style={view.percent === null ? undefined : { width: `${view.percent}%` }} />
      </div>
      <p className="help">{view.detail}</p>
    </section>
  );
}
