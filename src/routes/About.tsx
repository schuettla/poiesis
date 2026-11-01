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

// The website's account of the same thing (website/index.html, "The idea" and
// the sections after it), in the app's own register.
const HARNESS_CELLS = [
  { title: "Remembers", text: "Facts about you and lessons from its mistakes, kept as plain files on your disk." },
  { title: "Reflects", text: "When a chat ends it reads it back. A lesson is kept only if it's worth keeping." },
  { title: "Repairs", text: "A file it can't read is set aside, not deleted. A tool that keeps failing is planned around." },
  { title: "Checks itself", text: "A change that makes it worse is put back, and you're told." },
];

const HARNESS_POINTS = [
  { lead: "Plans, then works.", text: "A big job gets a plan it ticks off. A wide one is handed to helper agents you can open, steer or stop." },
  { lead: "Asks before it acts.", text: "Reading runs, a change asks, a send asks every time. Folders are deny-by-default." },
  { lead: "Shows what it changed.", text: "Edits come as a patch you can undo, and every write to its own memory shows a notice with an Undo." },
  { lead: "Works with other agents.", text: "Claude Code, Codex and OpenCode can take a job, and their questions come to Poiesis's own panel." },
  { lead: "Local first.", text: "Models run on your machine. Cloud models need your own key and are always marked." },
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
        <h1>About</h1>

        <section className="about-hero">
          <div className="about-mark">
            <PoiesisMark size={72} />
          </div>
          <div>
            <h2 className="about-name">Poiesis Agent</h2>
            <p className="about-tagline">The agent that tends to itself.</p>
            <p className="about-meta">
              {version ? `Version ${version}` : "Browser preview"} · Windows desktop · MIT license
            </p>
            <button className="settings-self-link" onClick={() => setView("workingdir")}>
              See what's in my working folder, and how big it is →
            </button>
          </div>
        </section>

        <section className="setting-block about-split">
          <div>
            <h2 className="setting-title">About the name</h2>
            <p className="setting-help">
              <strong>Poiesis</strong> (ποίησις) is Greek for making: bringing something into
              being that wasn't there before. It's the root of "poetry". Say it:{" "}
              <em>poy-EE-sis</em>.
            </p>
            <p className="setting-help">
              Autopoiesis, a word from the biologists Maturana and Varela, is a system that keeps
              making the parts it's made of. I try to do both: make things with you, and keep
              making myself.
            </p>
          </div>
          <figure className="about-plate">
            <img
              src="/illustrations/04-athena-olive.webp"
              width={552}
              height={552}
              alt="Ink drawing of Athena's olive tree on the Acropolis, a ring of stones around the trunk and its roots spreading below the ground as a network."
            />
            <figcaption>Athena's olive tree: roots below, a ring of stones around it.</figcaption>
          </figure>
        </section>

        <section className="setting-block">
          <h2 className="setting-title">The harness</h2>
          <p className="setting-help">
            The model only answers. The harness is everything around it: it decides what each
            turn does, runs the tools, asks before it acts, and keeps a record. It runs in the
            app itself, so it works the same whichever model you pick.
          </p>
          <div className="about-cells">
            {HARNESS_CELLS.map((c, i) => (
              <div key={c.title} className="about-cell">
                <span className="about-cell-n">{String(i + 1).padStart(2, "0")}</span>
                <h3>{c.title}</h3>
                <p>{c.text}</p>
              </div>
            ))}
          </div>
          <ul className="about-points">
            {HARNESS_POINTS.map((p) => (
              <li key={p.lead}>
                <span>
                  <strong>{p.lead}</strong> {p.text}
                </span>
              </li>
            ))}
          </ul>
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
