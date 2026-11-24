import { useState } from "react";
import Art from "../components/Art";
import { useStore } from "../store";
import { words } from "../link";

/** First start: scan the code on the desktop, or paste its text. Fixed size, so it never jumps. */
export default function Pair() {
  const { pairWith, error } = useStore();
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState("My phone");
  const [text, setText] = useState("");
  const [pasting, setPasting] = useState(false);
  const [local, setLocal] = useState<string | null>(null);

  async function run(payload: string) {
    setBusy(true);
    setLocal(null);
    await pairWith(payload, name);
    setBusy(false);
  }

  async function scanIt() {
    setLocal(null);
    try {
      // Loaded here: the scanner exists only inside the phone app.
      const scanner = await import("@tauri-apps/plugin-barcode-scanner");
      let allowed = (await scanner.checkPermissions()) === "granted";
      if (!allowed) allowed = (await scanner.requestPermissions()) === "granted";
      if (!allowed) {
        setLocal("Poiesis needs the camera to scan the code. You can paste the text instead.");
        return;
      }
      const found = await scanner.scan({ windowed: false, formats: [scanner.Format.QRCode] });
      await run(found.content);
    } catch (e) {
      setLocal(words(e));
    }
  }

  const problem = local ?? error;
  return (
    <section className="screen pair">
      <div className="pair-card">
        <Art name="amphora" className="pair-art" />
        <h1>Poiesis, in your pocket</h1>
        <p>
          On your computer open Settings, then Remote, and choose <strong>Pair a phone</strong>. Then scan the code it
          shows.
        </p>
        <label className="field">
          <span>Name this phone</span>
          <input value={name} maxLength={40} onChange={(e) => setName(e.target.value)} />
        </label>
        {pasting ? (
          <>
            <label className="field">
              <span>Pairing text from the desktop</span>
              <textarea rows={2} value={text} autoFocus onChange={(e) => setText(e.target.value)} />
            </label>
            <button className="btn btn-primary btn-wide" disabled={busy || !text.trim()} onClick={() => void run(text)}>
              {busy ? "Pairing ..." : "Pair"}
            </button>
            <button className="btn btn-ghost btn-wide" onClick={() => setPasting(false)}>
              Scan the code instead
            </button>
          </>
        ) : (
          <>
            <button className="btn btn-primary btn-wide" disabled={busy} onClick={() => void scanIt()}>
              {busy ? "Pairing ..." : "Scan the code"}
            </button>
            <button className="btn btn-ghost btn-wide" onClick={() => setPasting(true)}>
              Paste the text instead
            </button>
          </>
        )}
        <p className="pair-problem" role="alert">
          {problem ?? ""}
        </p>
      </div>
    </section>
  );
}
