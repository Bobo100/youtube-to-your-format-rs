import { useState, type FormEvent } from "react";
import { lookup, type Lookup, type SaveFormat, type VideoCard } from "../api";
import { VideoCardView } from "../components/VideoCardView";
import { errorMessage, t } from "../i18n";
import { ListIcon, MusicIcon, SearchIcon } from "../icons";

type Props = {
  /** Undefined until saving is wired up: the save buttons render disabled. */
  onSave?: (cards: VideoCard[], format: SaveFormat) => void;
};

type State =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "done"; query: string; result: Lookup }
  | { status: "error"; code: string };

export function Home({ onSave }: Props) {
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
        <Results result={state.result} onSave={onSave} onWholePlaylist={() => run(state.query, true)} />
      )}
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
  onSave,
  onWholePlaylist,
}: {
  result: Lookup;
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
          <button className="btn" disabled={!onSave} onClick={() => onSave?.(result.items, "audio")}>
            <MusicIcon />
            {t("saveAllAudio")}
          </button>
        </div>
      )}
      {result.kind === "search" && skippedNote}
      {result.items.map((card, index) => (
        // Hand-made playlists may contain the same video twice.
        <VideoCardView
          key={`${index}-${card.id}`}
          card={card}
          onSave={onSave && ((c, format) => onSave([c], format))}
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
