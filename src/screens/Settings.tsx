import { open } from "@tauri-apps/plugin-dialog";
import { useRef, type KeyboardEvent } from "react";
import type { SettingsPatch, SettingsView } from "../api";
import { dirLabel, errorMessage, t, type MessageKey } from "../i18n";
import { FolderIcon } from "../icons";

type Props = {
  view: SettingsView;
  error: string | null;
  onChange: (patch: SettingsPatch) => void;
};

type Option<T> = { value: T; label: MessageKey; text?: string };

function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: MessageKey;
  value: T;
  options: Option<T>[];
  onChange: (value: T) => void;
}) {
  const id = `setting-${label}`;
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  // Radio group keyboard pattern: arrows move and select, Tab enters at the selected one.
  const onKey = (event: KeyboardEvent, index: number) => {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[event.key];
    if (!step) return;
    event.preventDefault();
    const next = (index + step + options.length) % options.length;
    onChange(options[next].value);
    buttons.current[next]?.focus();
  };
  return (
    <div className="set">
      <span id={id}>{t(label)}</span>
      <div className="seg" role="radiogroup" aria-labelledby={id}>
        {options.map((option, index) => (
          <button
            key={option.value}
            ref={(el) => {
              buttons.current[index] = el;
            }}
            role="radio"
            aria-checked={value === option.value}
            tabIndex={value === option.value ? 0 : -1}
            className={value === option.value ? "on" : undefined}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => onKey(event, index)}
          >
            {option.text ?? t(option.label)}
          </button>
        ))}
      </div>
    </div>
  );
}

export function Settings({ view, error, onChange }: Props) {
  const { settings } = view;
  const chosenMissing = settings.outputDir !== null && settings.outputDir !== view.outputDir;

  const chooseFolder = () => {
    open({ directory: true, defaultPath: view.outputDir })
      .then((picked) => {
        if (typeof picked === "string") onChange({ outputDir: picked });
      })
      .catch((err: unknown) => console.error("folder dialog failed", err));
  };

  return (
    <>
      <div className="set column">
        <span>{t("settingFolder")}</span>
        <p className="status">{dirLabel(view.outputDir)}</p>
        {chosenMissing && <p className="warn">{t("settingFolderMissing", { folder: dirLabel(view.outputDir) })}</p>}
        <div className="acts">
          <button className="btn sec" onClick={chooseFolder}>
            <FolderIcon />
            {t("chooseFolder")}
          </button>
          {settings.outputDir !== null && (
            <button className="btn sec" onClick={() => onChange({ outputDir: "" })}>
              {t("useDefaultFolder")}
            </button>
          )}
        </div>
      </div>
      <Segmented
        label="settingFontSize"
        value={settings.fontSize}
        options={[
          { value: "large", label: "fontLarge" },
          { value: "xlarge", label: "fontXlarge" },
        ]}
        onChange={(fontSize) => onChange({ fontSize })}
      />
      <Segmented
        label="settingTheme"
        value={settings.theme}
        options={[
          { value: "light", label: "themeLight" },
          { value: "dark", label: "themeDark" },
        ]}
        onChange={(theme) => onChange({ theme })}
      />
      <Segmented
        label="settingLanguage"
        value={settings.language}
        options={[
          // Language names stay in their own language so either can be found.
          { value: "zh", label: "settingLanguage", text: "中文" },
          { value: "en", label: "settingLanguage", text: "English" },
        ]}
        onChange={(language) => onChange({ language })}
      />
      {error && (
        <p className="warn" role="alert">
          {errorMessage(error, "error.settings_save_failed")}
        </p>
      )}
      <p className="help">
        {t("versionLine", { app: view.appVersion, ytdlp: view.ytdlpVersion ?? t("unknownVersion") })}
      </p>
    </>
  );
}
