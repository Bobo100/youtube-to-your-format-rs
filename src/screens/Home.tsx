import { useState, type FormEvent } from "react";
import { lookup, type Lookup, type SaveFormat, type VideoCard } from "../api";
import { VideoCardView } from "../components/VideoCardView";
import { errorMessage, t } from "../i18n";
import { ListIcon, MusicIcon, SearchIcon } from "../icons";

type Props = {
  onSave: (cards: VideoCard[], format: SaveFormat) => void;
};

type State =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "done"; result: Lookup }
  | { status: "error"; code: string };

export function Home({ onSave }: Props) {
  const [text, setText] = useState("");
  const [state, setState] = useState<State>({ status: "idle" });

  const run = (wholePlaylist: boolean) => {
    if (!text.trim()) return;
    setState({ status: "loading" });
    lookup(text, wholePlaylist)
      .then((result) => setState({ status: "done", result }))
      .catch((code: unknown) => setState({ status: "error", code: String(code) }));
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    run(false);
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
          <button className="btn" type="submit" disabled={state.status === "loading"}>
            <SearchIcon />
            {t("find")}
          </button>
        </div>
        <p className="help">{t("homeHelp")}</p>
      </form>

      {state.status === "loading" && (
        <div className="res" aria-live="polite">
          <p className="status">{t("finding")}</p>
          <div className="bar indeterminate" role="progressbar">
            <i />
          </div>
        </div>
      )}

      {state.status === "error" && (
        <p className="res warn" role="alert">
          {errorMessage(state.code)}
        </p>
      )}

      {state.status === "done" && (
        <Results result={state.result} onSave={onSave} onWholePlaylist={() => run(true)} />
      )}
    </>
  );
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
  if (result.items.length === 0) {
    return <p className="res status">{t("noResults")}</p>;
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
          <button className="btn" onClick={() => onSave(result.items, "audio")}>
            <MusicIcon />
            {t("saveAllAudio")}
          </button>
        </div>
      )}
      {result.items.map((card) => (
        <VideoCardView key={card.id} card={card} onSave={(c, format) => onSave([c], format)} />
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
