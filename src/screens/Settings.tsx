import { open } from "@tauri-apps/plugin-dialog";
import type { AppSettings, SettingsView } from "../api";
import { dirLabel, errorMessage, t, type MessageKey } from "../i18n";
import { FolderIcon } from "../icons";

type Props = {
  view: SettingsView;
  error: string | null;
  onChange: (settings: AppSettings) => void;
};

type Option<T> = { value: T; label: MessageKey };

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
  return (
    <div className="set">
      <span id={id}>{t(label)}</span>
      <div className="seg" role="radiogroup" aria-labelledby={id}>
        {options.map((option) => (
          <button
            key={option.value}
            role="radio"
            aria-checked={value === option.value}
            className={value === option.value ? "on" : undefined}
            onClick={() => onChange(option.value)}
          >
            {t(option.label)}
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
        if (typeof picked === "string") onChange({ ...settings, outputDir: picked });
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
            <button className="btn sec" onClick={() => onChange({ ...settings, outputDir: null })}>
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
        onChange={(fontSize) => onChange({ ...settings, fontSize })}
      />
      <Segmented
        label="settingTheme"
        value={settings.theme}
        options={[
          { value: "light", label: "themeLight" },
          { value: "dark", label: "themeDark" },
        ]}
        onChange={(theme) => onChange({ ...settings, theme })}
      />
      <div className="set">
        <span id="setting-language">{t("settingLanguage")}</span>
        <div className="seg" role="radiogroup" aria-labelledby="setting-language">
          {/* Language names stay in their own language so either can be found. */}
          <button role="radio" aria-checked={settings.language === "zh"} className={settings.language === "zh" ? "on" : undefined} onClick={() => onChange({ ...settings, language: "zh" })}>
            中文
          </button>
          <button role="radio" aria-checked={settings.language === "en"} className={settings.language === "en" ? "on" : undefined} onClick={() => onChange({ ...settings, language: "en" })}>
            English
          </button>
        </div>
      </div>
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
