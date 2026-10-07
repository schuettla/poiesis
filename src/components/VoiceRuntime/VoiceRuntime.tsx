import { useEffect, useRef, useState } from "react";
import {
  setSetting,
  voiceCatalog,
  voiceDelete,
  voiceDownload,
  voicePreview,
  voiceSettings,
  voiceStatus,
  type DownloadProgress,
  type VoiceCatalog,
  type VoiceSettings,
  type VoiceStatus,
} from "../../lib/api";
import { VoiceAudio } from "../../lib/voice/audio";
import Select, { type SelectOption } from "../Select/Select";
import { DEFAULT_VOICE_HOTKEY, hotkeyFromEvent, setVoiceHotkey } from "../../lib/voice/hotkey";
import "./VoiceRuntime.css";

/** What a voice says when asked to "Hear it", in its own language. */
const SAMPLE: Record<string, string> = {
  en: "Hello, I'm Poiesis. This is how I sound.",
  de: "Hallo, ich bin Poiesis. So klinge ich.",
  fr: "Bonjour, je suis Poiesis. Voici ma voix.",
  es: "Hola, soy Poiesis. Así es como sueno.",
  it: "Ciao, sono Poiesis. Ecco come suono.",
  nl: "Hallo, ik ben Poiesis. Zo klink ik.",
  pt: "Olá, eu sou o Poiesis. É assim que eu soo.",
  pl: "Cześć, jestem Poiesis. Tak brzmię.",
  ru: "Привет, я Poiesis. Вот как я звучу.",
  sv: "Hej, jag är Poiesis. Så här låter jag.",
  da: "Hej, jeg er Poiesis. Sådan lyder jeg.",
  no: "Hei, jeg er Poiesis. Slik høres jeg ut.",
  fi: "Hei, olen Poiesis. Tältä kuulostan.",
  cs: "Ahoj, jsem Poiesis. Takhle zním.",
  el: "Γεια σου, είμαι το Poiesis. Έτσι ακούγομαι.",
  hu: "Szia, én vagyok a Poiesis. Így hangzom.",
  ro: "Salut, eu sunt Poiesis. Așa sun.",
  vi: "Xin chào, tôi là Poiesis. Đây là giọng của tôi.",
};

const names = new Intl.DisplayNames(["en"], { type: "language" });
const regionNames = new Intl.DisplayNames(["en"], { type: "region" });

/** English name of a language code ("de" gives "German"). */
function languageName(code: string): string {
  try {
    return names.of(code) ?? code;
  } catch {
    return code;
  }
}

/** The name in English, and in the language itself when that differs, so a
 * person can find their own language in either ("French · Français"). */
function languageLabel(code: string): string {
  try {
    const raw = new Intl.DisplayNames([code], { type: "language" }).of(code);
    // Some languages write their own name in lower case ("français").
    const own = raw && raw.charAt(0).toLocaleUpperCase(code) + raw.slice(1);
    const english = languageName(code);
    return own && own.toLowerCase() !== english.toLowerCase() ? `${english} · ${own}` : english;
  } catch {
    return languageName(code);
  }
}

/** The country a voice is from, only for a language that has voices from more
 * than one (English from the UK and the US). */
function regionTag(locale: string, byLanguage: Map<string, Set<string>>): string | null {
  const [language, region] = locale.split("_");
  if (!region || (byLanguage.get(language)?.size ?? 0) < 2) return null;
  try {
    return regionNames.of(region) ?? region;
  } catch {
    return region;
  }
}

/** The "Voice" tab of the Runtime view (`VOC-UI-6`): what Poiesis hears with and
 * speaks with, how it sounds, and a check that the microphone works. Everything
 * stays on this computer. Copy follows VXP-6: voice, hearing, listening. */
export default function VoiceRuntime() {
  const [catalog, setCatalog] = useState<VoiceCatalog | null>(null);
  const [status, setStatus] = useState<VoiceStatus | null>(null);
  const [settings, setSettings] = useState<VoiceSettings | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [prog, setProg] = useState<DownloadProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [playing, setPlaying] = useState<string | null>(null);
  const [filter, setFilter] = useState<string | null>(null);
  const player = useRef<HTMLAudioElement | null>(null);

  async function refresh() {
    try {
      const [c, s, v] = await Promise.all([voiceCatalog(), voiceStatus(), voiceSettings()]);
      setCatalog(c);
      setStatus(s);
      setSettings(v);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    void refresh();
    return () => player.current?.pause();
  }, []);

  /** Runs one download or removal and re-reads the real state afterwards. */
  async function work(label: string, job: () => Promise<unknown>) {
    setBusy(label);
    setError(null);
    try {
      await job();
    } catch (e) {
      setError(typeof e === "string" ? e : "That did not work. Try again.");
    } finally {
      await refresh();
      setBusy(null);
      setProg(null);
    }
  }

  async function save(key: string, value: string) {
    setError(null);
    try {
      await setSetting(key, value);
      setSettings(await voiceSettings());
    } catch (e) {
      setError(String(e));
    }
  }

  async function play(voiceId: string, language: string) {
    player.current?.pause();
    setPlaying(voiceId);
    setError(null);
    try {
      const wav = await voicePreview(voiceId, SAMPLE[language] ?? SAMPLE.en);
      const url = URL.createObjectURL(new Blob([wav], { type: "audio/wav" }));
      const audio = new Audio(url);
      player.current = audio;
      audio.onended = () => {
        URL.revokeObjectURL(url);
        setPlaying(null);
      };
      await audio.play();
    } catch (e) {
      setPlaying(null);
      setError(typeof e === "string" ? e : "That voice could not be played.");
    }
  }

  const pct = prog?.total ? Math.round((prog.received / prog.total) * 100) : null;
  const installedHearings = status?.hearings ?? [];
  const installedVoices = status?.voices ?? [];
  const installedVoiceCount = (catalog?.voices ?? [])
    .filter((m) => installedVoices.includes(m.id))
    .reduce((n, m) => n + m.voices.length, 0);

  const allVoices = (catalog?.voices ?? []).flatMap((model) =>
    model.voices.map((voice) => ({ model, voice, installed: installedVoices.includes(model.id) })),
  );
  const languages = [...new Set(allVoices.map((r) => r.voice.language))].sort((a, b) =>
    languageName(a).localeCompare(languageName(b)),
  );
  const regionsByLanguage = new Map<string, Set<string>>();
  for (const { voice } of allVoices) {
    if (!regionsByLanguage.has(voice.language)) regionsByLanguage.set(voice.language, new Set());
    regionsByLanguage.get(voice.language)!.add(voice.locale);
  }
  // Start on the language of the voice in use, so the person sees their own first.
  const activeLanguage = allVoices.find((r) => r.voice.id === settings?.voice_id)?.voice.language;
  const shownFilter = filter ?? activeLanguage ?? "all";
  const rows = allVoices.filter((r) =>
    shownFilter === "all" ? true : shownFilter === "installed" ? r.installed : r.voice.language === shownFilter,
  );
  const filterOptions: SelectOption[] = [
    { value: "installed", label: "Installed voices", hint: String(installedVoiceCount) },
    { value: "all", label: "All languages", hint: String(allVoices.length) },
    ...languages.map((l) => ({
      value: l,
      label: languageLabel(l),
      hint: String(allVoices.filter((r) => r.voice.language === l).length),
    })),
  ];
  const languageOptions: SelectOption[] = [
    { value: "auto", label: "Follow my language" },
    ...languages.map((l) => ({ value: l, label: languageLabel(l) })),
  ];

  return (
    <>
      {error && <p className="hw-note error">{error}</p>}

      {catalog?.hearing.map((hearing) => {
        const installed = installedHearings.includes(hearing.id);
        // With more than one installed, the person picks which one is used.
        const chosen = (settings?.hearing_model ?? "") === hearing.id;
        return (
          <section className="runtime-card" key={hearing.id}>
            <div className="runtime-card-head">
              <h2 className="section-title">{hearing.name}</h2>
              <span className={`runtime-state-badge ${installed ? "running" : "idle"}`}>
                <span className="dot" aria-hidden="true" />
                {installed ? "Installed" : "Not installed"}
              </span>
            </div>
            <p className="runtime-sub">
              This is how I understand what you say. It runs on your computer and never sends your voice anywhere.{" "}
              {hearing.note}
            </p>
            {busy === hearing.id && prog ? (
              <div className="dl-progress wide" style={{ marginTop: 12 }}>
                <div className="dl-bar" style={{ width: pct !== null ? `${pct}%` : "40%" }} />
                <span className="dl-pct">{prog.label}</span>
              </div>
            ) : (
              <div className="runtime-actions">
                {installed ? (
                  <>
                    {installedHearings.length > 1 && (
                      <button
                        className="btn-secondary"
                        aria-pressed={chosen}
                        disabled={chosen}
                        onClick={() => void save("voice.hearing_model", hearing.id)}
                      >
                        {chosen ? "In use" : "Use this one"}
                      </button>
                    )}
                    <button
                      className="btn-secondary"
                      disabled={!!busy}
                      onClick={() => void work(hearing.id, () => voiceDelete("hearing", hearing.id))}
                    >
                      Remove
                    </button>
                  </>
                ) : (
                  <button
                    className="btn-primary"
                    disabled={!!busy}
                    onClick={() =>
                      void work(hearing.id, async () => {
                        await voiceDownload("hearing", hearing.id, setProg);
                        // The first hearing is used at once. A second one waits until the
                        // person picks it: the light one knows English only.
                        if (installedHearings.length === 0) await setSetting("voice.hearing_model", hearing.id);
                      })
                    }
                  >
                    Download ({hearing.size_label.toLowerCase()})
                  </button>
                )}
              </div>
            )}
          </section>
        );
      })}

      <section className="runtime-card">
        <div className="runtime-card-head">
          <h2 className="section-title">Voices</h2>
          <span className={`runtime-state-badge ${installedVoiceCount > 0 ? "running" : "idle"}`}>
            <span className="dot" aria-hidden="true" />
            {installedVoiceCount} installed
          </span>
        </div>
        <p className="runtime-sub">
          Pick the voice I speak with. A voice downloads once and then works without a connection. Voices come
          in many languages; get the ones you need.
        </p>
        <div className="voice-rt-filter">
          <Select
            label="Which voices to show"
            value={shownFilter}
            options={filterOptions}
            onChange={setFilter}
          />
        </div>
        <div className="backend-list voice-rt-voices">
          {rows.map(({ model, voice, installed }) => {
            const active = settings?.voice_id === voice.id;
            const working = busy === model.id;
            const set = model.voices.length > 1;
            return (
              <div key={voice.id} className={`backend-row voice-rt-row${active ? " active" : ""}`}>
                <button
                  className="voice-rt-pick"
                  aria-pressed={active}
                  disabled={!installed}
                  title={installed ? undefined : "Download it first"}
                  onClick={() => void save("voice.voice_id", voice.id)}
                >
                  <span className="backend-radio" aria-hidden="true" />
                  <span className="voice-rt-who">
                    <span className="backend-name">{voice.name}</span>
                    <span className="voice-rt-meta">
                      {[
                        regionTag(voice.locale, regionsByLanguage),
                        model.license,
                        installed ? null : model.size_label.replace("About ", ""),
                        set ? `set of ${model.voices.length}` : null,
                      ]
                        .filter(Boolean)
                        .join(" · ")}
                    </span>
                  </span>
                </button>
                {working && prog ? (
                  <div className="dl-progress voice-rt-dl">
                    <div className="dl-bar" style={{ width: pct !== null ? `${pct}%` : "40%" }} />
                    <span className="dl-pct">{prog.label}</span>
                  </div>
                ) : installed ? (
                  <>
                    <button
                      className="btn-secondary voice-rt-play"
                      disabled={playing === voice.id}
                      onClick={() => void play(voice.id, voice.language)}
                      aria-label={`Hear ${voice.name}`}
                    >
                      {playing === voice.id ? "Playing…" : "Hear it"}
                    </button>
                    <button
                      className="voice-rt-remove"
                      disabled={!!busy}
                      title={set ? "Removes every voice in this set" : "Remove this voice"}
                      aria-label={`${set ? "Remove the set with" : "Remove"} ${voice.name}`}
                      onClick={() =>
                        void work(model.id, async () => {
                          const left = await voiceDelete("voice", model.id);
                          // The voice in use went with it: carry on with one that is still here.
                          // Same language first.
                          const gone = model.voices.find((v) => v.id === settings?.voice_id);
                          const still = (catalog?.voices ?? []).filter((m) => left.voices.includes(m.id)).flatMap((m) => m.voices);
                          const next = still.find((v) => v.language === gone?.language) ?? still[0];
                          if (gone && next) await setSetting("voice.voice_id", next.id);
                        })
                      }
                    >
                      {set ? "Remove set" : "Remove"}
                    </button>
                  </>
                ) : (
                  <button
                    className="btn-secondary voice-rt-get"
                    disabled={!!busy}
                    aria-label={`Download ${voice.name}`}
                    onClick={() => void work(model.id, () => voiceDownload("voice", model.id, setProg))}
                  >
                    Download
                  </button>
                )}
              </div>
            );
          })}
          {rows.length === 0 && <p className="runtime-hint">No voices are installed yet. Pick a language above.</p>}
        </div>
      </section>

      {settings && (
        <section className="runtime-card">
          <h2 className="section-title">How I talk with you</h2>
          <div className="voice-rt-field">
            <label htmlFor="voice-rt-speed">Speaking speed</label>
            <input
              id="voice-rt-speed"
              className="voice-rt-slider"
              type="range"
              min={0.8}
              max={1.3}
              step={0.05}
              value={settings.speed}
              style={{ "--fill": `${((settings.speed - 0.8) / 0.5) * 100}%` } as React.CSSProperties}
              onChange={(e) => setSettings({ ...settings, speed: Number(e.target.value) })}
              onPointerUp={() => void save("voice.speed", String(settings.speed))}
              onKeyUp={() => void save("voice.speed", String(settings.speed))}
            />
            <span className="voice-rt-value">{settings.speed.toFixed(2)}×</span>
          </div>
          <div className="voice-rt-field">
            <label htmlFor="voice-rt-language">Language</label>
            <Select
              id="voice-rt-language"
              label="Language"
              value={settings.language}
              options={languageOptions}
              onChange={(v) => void save("voice.language", v)}
            />
          </div>
          <HotkeyField
            value={settings.hotkey}
            onPick={(spec) => {
              setVoiceHotkey(spec);
              void save("voice.hotkey", spec);
            }}
          />
          <label className="voice-rt-check">
            <input
              type="checkbox"
              checked={settings.cut_in}
              onChange={(e) => void save("voice.cut_in", e.target.checked ? "on" : "off")}
            />
            <span>
              Let me cut in. Speak while I am talking and I stop. Turn this off if I stop by mistake, for example
              on speakers; the Stop button and Escape always work.
            </span>
          </label>
        </section>
      )}

      <MicTest />
    </>
  );
}

/** The key that starts and ends a voice conversation. Press "Change", then the
 * new combination. Escape gives up. It works while Poiesis has the focus. */
function HotkeyField({ value, onPick }: { value: string; onPick: (spec: string) => void }) {
  const [picking, setPicking] = useState(false);

  useEffect(() => {
    if (!picking) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setPicking(false);
        return;
      }
      const spec = hotkeyFromEvent(e);
      if (!spec) return;
      setPicking(false);
      onPick(spec);
    };
    // Capture phase: the app's own shortcuts must not see this key press.
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [picking, onPick]);

  return (
    <div className="voice-rt-field">
      <span className="voice-rt-label">Shortcut</span>
      <kbd className="voice-rt-key">{picking ? "Press the keys…" : value}</kbd>
      <button className="btn-secondary" onClick={() => setPicking(!picking)}>
        {picking ? "Cancel" : "Change"}
      </button>
      {!picking && value !== DEFAULT_VOICE_HOTKEY && (
        <button className="btn-secondary" onClick={() => onPick(DEFAULT_VOICE_HOTKEY)}>
          Reset
        </button>
      )}
    </div>
  );
}

/** Opens the mic for a moment and shows how loud it hears you. It closes when
 * stopped or when the page is left (`VXP-3`). */
function MicTest() {
  const [on, setOn] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [level, setLevel] = useState(0);
  const audio = useRef<VoiceAudio | null>(null);

  function stop() {
    audio.current?.stop();
    audio.current = null;
    setOn(false);
    setLevel(0);
  }
  useEffect(() => stop, []);

  async function start() {
    setNote(null);
    const a = new VoiceAudio({ onLevel: (input) => setLevel(Math.min(1, input * 6)) });
    try {
      await a.start({ playback: false });
      audio.current = a;
      setOn(true);
    } catch (e) {
      setNote(e instanceof Error ? e.message : "The microphone could not be started.");
    }
  }

  return (
    <section className="runtime-card">
      <div className="runtime-card-head">
        <h2 className="section-title">Microphone</h2>
        {on && <span className="runtime-state-badge running">Listening</span>}
      </div>
      <p className="runtime-sub">Say something. The bar moves when I can hear you.</p>
      {note && <p className="hw-note error">{note}</p>}
      <div className="voice-rt-meter" aria-hidden="true">
        <div className="voice-rt-meter-fill" style={{ width: `${Math.round(level * 100)}%` }} />
      </div>
      <div className="runtime-actions">
        <button className="btn-secondary" onClick={on ? stop : () => void start()}>
          {on ? "Stop listening" : "Test the microphone"}
        </button>
      </div>
    </section>
  );
}
