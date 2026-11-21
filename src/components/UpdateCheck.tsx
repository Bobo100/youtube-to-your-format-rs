import { useEffect, useReducer } from "react";
import { checkAppUpdate, installAppUpdateNow, onAppUpdateProgress } from "../api";
import { checkEnabled, reduceUpdate, type UpdateState } from "../appUpdate";
import { errorMessage, t } from "../i18n";

const START: UpdateState = { kind: "idle" };

export function UpdateCheck({ queueBusy }: { queueBusy: boolean }) {
  const [state, dispatch] = useReducer(reduceUpdate, START);

  useEffect(() => {
    let cancelled = false;
    let stop: (() => void) | undefined;
    onAppUpdateProgress((p) => dispatch({ type: "progress", received: p.received, total: p.total })).then((unlisten) => {
      if (cancelled) unlisten();
      else stop = unlisten;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  const check = () => {
    dispatch({ type: "check" });
    checkAppUpdate()
      .then((version) => dispatch({ type: "checked", version }))
      .catch((code: unknown) => dispatch({ type: "failed", code: String(code) }));
  };

  const install = () => {
    dispatch({ type: "install" });
    installAppUpdateNow()
      .then((installed) => installed || dispatch({ type: "failed", code: "update_failed" }))
      .catch((code: unknown) => dispatch({ type: "failed", code: String(code) }));
  };

  return (
    <div className="set column">
      <span>{t("checkUpdate")}</span>
      <div className="acts">
        <button className="btn sec" onClick={check} disabled={!checkEnabled(state, queueBusy)}>
          {t("checkUpdate")}
        </button>
        {state.kind === "available" && (
          <button className="btn" onClick={install} disabled={queueBusy}>
            {t("installUpdate")}
          </button>
        )}
      </div>
      {queueBusy && state.kind !== "installing" && <p className="help">{t("updateBusyHint")}</p>}
      <div role="status">
        {state.kind === "checking" && <p className="status">{t("checkingUpdate")}</p>}
        {state.kind === "upToDate" && <p className="status">{t("upToDate")}</p>}
        {state.kind === "available" && (
          <>
            <p className="status">{t("updateAvailable", { version: state.version })}</p>
            <p className="help">{t("installUpdateHint")}</p>
          </>
        )}
        {state.kind === "installing" && (
          <p className="status">
            {t("installingUpdate", { version: state.version })}
            {state.percent !== null && ` ${state.percent}%`}
          </p>
        )}
      </div>
      {state.kind === "failed" && (
        <p className="warn" role="alert">
          {errorMessage(state.code, "error.update_failed")}
        </p>
      )}
    </div>
  );
}
