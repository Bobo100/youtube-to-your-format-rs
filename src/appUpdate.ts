export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate" }
  | { kind: "available"; version: string }
  | { kind: "installing"; version: string; percent: number | null }
  | { kind: "failed"; code: string };

export type UpdateEvent =
  | { type: "check" }
  | { type: "checked"; version: string | null }
  | { type: "install" }
  | { type: "progress"; received: number; total: number | null }
  | { type: "failed"; code: string };

export function reduceUpdate(state: UpdateState, event: UpdateEvent): UpdateState {
  switch (event.type) {
    case "check":
      return state.kind === "checking" || state.kind === "installing" ? state : { kind: "checking" };
    case "checked":
      if (state.kind !== "checking") return state;
      return event.version === null ? { kind: "upToDate" } : { kind: "available", version: event.version };
    case "install":
      return state.kind === "available" ? { kind: "installing", version: state.version, percent: null } : state;
    case "progress": {
      if (state.kind !== "installing") return state;
      const percent = event.total ? Math.min(Math.round((event.received / event.total) * 100), 100) : null;
      return { ...state, percent };
    }
    case "failed":
      return { kind: "failed", code: event.code };
  }
}

/** The check is blocked while downloads run: the installer closes the app. */
export function checkEnabled(state: UpdateState, queueBusy: boolean): boolean {
  return !queueBusy && state.kind !== "checking" && state.kind !== "installing";
}
