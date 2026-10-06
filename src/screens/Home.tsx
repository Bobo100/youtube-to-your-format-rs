import { useState, type FormEvent } from "react";
import { lookup, type Job, type Lookup, type SaveFormat, type VideoCard } from "../api";
import { OtherJobs } from "../components/OtherJobs";
import { VideoCardView } from "../components/VideoCardView";
import { errorMessage, t } from "../i18n";
import { ListIcon, MusicIcon, SearchIcon } from "../icons";
import { latestJobFor, offScreenJobs } from "../jobs";

type Props = {
  jobs: Job[];
  /** `skipDone` is set by the batch button so saved songs are not fetched again. */
  onSave: (cards: VideoCard[], format: SaveFormat, skipDone?: boolean) => Promise<void>;
};

type State =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "done"; query: string; result: Lookup }
  | { status: "error"; code: string };

export function Home({ jobs, onSave }: Props) {
  const [text, setText] = useState("");
  const [state, setState] = useState<State>({ status: "idle" });
  const loading = state.status === "loading";

  const run = (query: string, wholePlaylist: boolean) => {
    if (!query.trim() || loading) return;
    setState({ status: "loading" });
    lookup(query, wholePlaylist)
      .then((result) => setState({ status: "done", query, result }))
      .catch((code: unknown) => setState({ status: "error", code: String(code) }));
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    run(text, false);
  };

  return (
    <>
      <form onSubmit={submit}>
        <label className="lbl" htmlFor="what">
          {t("homeLabel")}
        </label>
        <div className="row">
          <input
            id="what"
            className="inp"
            value={text}
            onChange={(event) => setText(event.target.value)}
            autoFocus
            autoComplete="off"
          />
          {/* aria-disabled instead of disabled: a disabled button drops keyboard focus. */}
          <button className="btn" type="submit" aria-disabled={loading}>
            <SearchIcon />
            {t("find")}
          </button>
        </div>
        <p className="help">{t("homeHelp")}</p>
      </form>

      <p className="visually-hidden" aria-live="polite">
        {announcement(state)}
      </p>

      {loading && (
        <div className="res">
          <p className="status">{t("finding")}</p>
          <div className="bar indeterminate" role="progressbar" aria-label={t("finding")}>
            <i />
          </div>
        </div>
      )}

      {state.status === "error" && (
        <p className="res warn" role="alert">
          {errorMessage(state.code, "error.lookup_failed")}
        </p>
      )}

      {state.status === "done" && (
        <Results result={state.result} jobs={jobs} onSave={onSave} onWholePlaylist={() => run(state.query, true)} />
      )}

      <OtherJobs
        jobs={offScreenJobs(jobs, state.status === "done" ? state.result.items.map((card) => card.id) : [])}
        onRetry={(job) => onSave([{ id: job.videoId, url: job.url, title: job.title, channel: null, durationS: null, thumbnail: "" }], job.format)}
      />
    </>
  );
}

function announcement(state: State): string {
  if (state.status === "loading") return t("finding");
  if (state.status === "done") {
    return state.result.items.length === 0 ? t("noResults") : t("foundCount", { count: state.result.items.length });
  }
  return "";
}

function Results({
  result,
  jobs,
  onSave,
  onWholePlaylist,
}: {
  result: Lookup;
  jobs: Job[];
  onSave: Props["onSave"];
  onWholePlaylist: () => void;
}) {
  const skippedNote = result.skipped > 0 && <p className="help">{t("skippedCount", { count: result.skipped })}</p>;
  if (result.items.length === 0) {
    return (
      <section className="res">
        <p className="status">{t("noResults")}</p>
        {skippedNote}
      </section>
    );
  }
  return (
    <section className="res">
      {result.kind === "playlist" && (
        <div className="list-head">
          <p className="status">
            {result.title ? `${result.title} · ` : ""}
            {t("playlistCount", { count: result.items.length })}
          </p>
          {result.truncated && <p className="help">{t("playlistTruncated", { count: result.items.length })}</p>}
          {skippedNote}
          <SaveAllButton onClick={() => onSave(result.items, "audio", true)} />
        </div>
      )}
      {result.kind === "search" && skippedNote}
      {result.items.map((card, index) => (
        // Hand-made playlists may contain the same video twice.
        <VideoCardView
          key={`${index}-${card.id}`}
          card={card}
          job={latestJobFor(jobs, card.id)}
          onSave={(c, format) => onSave([c], format)}
        />
      ))}
      {result.hasPlaylist && (
        <button className="btn sec" onClick={onWholePlaylist}>
          <ListIcon />
          {t("wholePlaylist")}
        </button>
      )}
    </section>
  );
}

/** Guards against double clicks while the batch is being queued. */
function SaveAllButton({ onClick }: { onClick: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const click = () => {
    if (busy) return;
    setBusy(true);
    onClick().finally(() => setBusy(false));
  };
  return (
    <button className="btn" aria-disabled={busy} onClick={click}>
      <MusicIcon />
      {t("saveAllAudio")}
    </button>
  );
}
