import { useEffect, useState } from "react";
import { getAppVersion, getProblemFacts, inTauri } from "../lib/api";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useAppStore } from "../lib/store";
import PoiesisMark from "../components/Mark/PoiesisMark";
import UpdatesBlock from "../components/Updates/UpdatesBlock";
import { problemReportUrl } from "../lib/updates";
import "./Surface.css";
import "./Settings.css";

const ATTRIBUTIONS = [
  { name: "llama.cpp", license: "MIT", what: "Local model runtime (llama-server)" },
  { name: "Tauri", license: "MIT / Apache-2.0", what: "Desktop application shell" },
  { name: "React", license: "MIT", what: "User interface" },
  { name: "Newsreader, Inter, JetBrains Mono", license: "OFL / MIT", what: "Typefaces" },
  { name: "rusqlite / SQLite", license: "MIT / Public Domain", what: "Local storage + search" },
  { name: "sherpa-onnx", license: "Apache-2.0", what: "Voice: listening and speaking on this device" },
  { name: "Silero VAD", license: "MIT", what: "Voice: telling speech from silence" },
  { name: "NVIDIA Parakeet TDT 0.6B v3", license: "CC-BY-4.0", what: "Voice: understanding speech" },
  { name: "Kokoro", license: "Apache-2.0", what: "Voice: English voices" },
  { name: "Thorsten Voice, Alba", license: "CC0 / CC-BY-4.0", what: "Voice: German and British English voices" },
  { name: "Model weights", license: "Per-model (shown on each model)", what: "e.g. Llama Community, Apache-2.0" },
];

export default function About() {
  const [version, setVersion] = useState("");
  const setView = useAppStore((s) => s.setView);

  useEffect(() => {
    if (!inTauri()) return;
    getAppVersion().then(setVersion).catch(() => {});
  }, []);

  return (
    <div className="surface">
      <div className="surface-inner">
        <div className="about-mark">
          <PoiesisMark size={64} />
        </div>
        <h1>About</h1>
        <p className="lede">Details about this install, and the open-source software it's built on.</p>

        <section className="setting-block">
          <h2 className="setting-title">Poiesis Agent</h2>
          <p className="setting-help">A local-first, agentic desktop LLM application.</p>
          <p className="setting-readout">{version ? `Version ${version}` : "Browser preview"}</p>
          <p className="setting-readout">Platform: Windows desktop (Tauri)</p>
          <button className="settings-self-link" onClick={() => setView("workingdir")}>
            See what's in my working folder, and how big it is →
          </button>
        </section>

        <UpdatesBlock version={version} />

        {inTauri() && (
          <section className="setting-block">
            <h2 className="setting-title">Problems</h2>
            <p className="setting-help">Something not working? Tell me about it.</p>
            <p className="setting-help">
              This opens a new issue on GitHub with my version and your Windows version filled in.
              Nothing from your conversations or files is included.
            </p>
            <button
              className="btn-secondary"
              onClick={() => {
                getProblemFacts()
                  .then((f) => openUrl(problemReportUrl(f)))
                  .catch(() => {});
              }}
            >
              Report a problem
            </button>
          </section>
        )}

        <section className="setting-block">
          <h2 className="setting-title">Third-party licenses</h2>
          <p className="setting-help">
            Poiesis Agent is built on open-source software. Thank you to these projects.
          </p>
          <ul className="attribution-list">
            {ATTRIBUTIONS.map((a) => (
              <li key={a.name} className="attribution-row">
                <span className="attribution-name">{a.name}</span>
                <span className="attribution-what">{a.what}</span>
                <span className="attribution-license">{a.license}</span>
              </li>
            ))}
          </ul>
        </section>
      </div>
    </div>
  );
}
