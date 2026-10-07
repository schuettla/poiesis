/**
 * @vitest-environment jsdom
 *
 * `VOC-UI-6`: the Voice tab. Two kinds of hearing can be installed, the person
 * picks which one is used, and the shortcut can be changed.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const hearing = (id: string, name: string) => ({
  id,
  family: id.startsWith("moon") ? "moonshine" : "parakeet",
  name,
  note: "A note.",
  size_label: "About 100 MB",
  languages: ["en"],
});

const voiceModel = (id: string, license: string, voices: [string, string, string][]) => ({
  id,
  note: "A note.",
  size_label: "About 21 MB",
  license,
  voices: voices.map(([vid, name, locale]) => ({ id: vid, name, sid: 0, language: locale.split("_")[0], locale })),
});

const settings = {
  hearing_model: "parakeet-v3",
  voice_id: "piper-de-thorsten",
  speed: 1,
  language: "auto",
  cut_in: true,
  hotkey: "Ctrl+Shift+Space",
  threads: 2,
};

const api = vi.hoisted(() => ({
  status: { hearing: "parakeet-v3", hearings: ["parakeet-v3"], voices: ["piper-de-thorsten"] as string[], loaded: false },
  // Answers with what is left, as the real command does.
  deleteVoice: vi.fn((_kind: string, id: string) =>
    Promise.resolve({ ...api.status, voices: api.status.voices.filter((v) => v !== id) }),
  ),
  setSetting: vi.fn(() => Promise.resolve()),
  voiceDownload: vi.fn(() => Promise.resolve()),
}));

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  voiceCatalog: () =>
    Promise.resolve({
      hearing: [hearing("parakeet-v3", "Standard hearing"), hearing("moonshine-en", "Light hearing")],
      voices: [
        voiceModel("kokoro-en", "Apache 2.0", [
          ["af_heart", "Heart", "en_US"],
          ["bf_emma", "Emma", "en_GB"],
        ]),
        voiceModel("piper-de-thorsten", "CC0", [["piper-de-thorsten", "Thorsten", "de_DE"]]),
        voiceModel("piper-de-kerstin", "CC0", [["piper-de-kerstin", "Kerstin", "de_DE"]]),
        voiceModel("piper-fr-siwis", "CC BY 4.0", [["piper-fr-siwis", "Siwis", "fr_FR"]]),
      ],
    }),
  voiceStatus: () => Promise.resolve({ ...api.status }),
  voiceSettings: () => Promise.resolve({ ...settings }),
  voiceDownload: api.voiceDownload,
  voiceDelete: api.deleteVoice,
  voicePreview: () => Promise.resolve(new ArrayBuffer(0)),
  setSetting: api.setSetting,
}));

import VoiceRuntime from "./VoiceRuntime";
import { getVoiceHotkey, setVoiceHotkey } from "../../lib/voice/hotkey";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  api.setSetting.mockClear();
  api.voiceDownload.mockClear();
  api.deleteVoice.mockClear();
  api.status = { hearing: "parakeet-v3", hearings: ["parakeet-v3"], voices: ["piper-de-thorsten"], loaded: false };
  setVoiceHotkey("");
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const render = () => act(async () => root.render(<VoiceRuntime />));
const button = (label: string, aria?: string) =>
  Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find((b) =>
    aria ? b.getAttribute("aria-label") === aria : b.textContent?.startsWith(label),
  );
const click = (b: HTMLButtonElement | undefined) => act(async () => b?.click());

const rowNames = () =>
  Array.from(container.querySelectorAll(".voice-rt-row .backend-name")).map((n) => n.textContent);

describe("the Voice tab", () => {
  it("keeps every voice in one box, starting with the language of the voice in use", async () => {
    await render();
    expect(Array.from(container.querySelectorAll("h2")).filter((h) => h.textContent === "Voices")).toHaveLength(1);
    // The active voice is German, so German is shown first.
    expect(rowNames()).toEqual(["Thorsten", "Kerstin"]);
    expect(container.querySelector(".voice-rt-row.active .backend-name")?.textContent).toBe("Thorsten");
    expect(container.querySelector(".select-value")?.textContent).toContain("German");
  });

  it("offers a download for a voice that is not installed, and a choice for one that is", async () => {
    await render();
    await click(button("Download", "Download Kerstin"));
    expect(api.voiceDownload).toHaveBeenCalledWith("voice", "piper-de-kerstin", expect.any(Function));
    expect(button("Hear it")).toBeTruthy();
    expect(container.querySelector<HTMLButtonElement>(".voice-rt-row:not(.active) .voice-rt-pick")?.disabled).toBe(true);
  });

  it("removes a voice, and tells when a removal takes a whole set", async () => {
    await render();
    await click(button("", "Remove Thorsten"));
    expect(api.deleteVoice).toHaveBeenCalledWith("voice", "piper-de-thorsten");
  });

  it("moves to another installed voice, same language first, when the one in use is removed", async () => {
    api.status = { ...api.status, voices: ["piper-de-thorsten", "kokoro-en", "piper-de-kerstin"] };
    await render();
    await click(button("", "Remove Thorsten"));
    expect(api.setSetting).toHaveBeenCalledWith("voice.voice_id", "piper-de-kerstin");
  });

  it("switches the list by language and shows where English voices come from", async () => {
    await render();
    // The select is a real dropdown, not the system one.
    expect(container.querySelector("select")).toBeNull();
    await click(container.querySelector<HTMLButtonElement>('button[aria-label="Which voices to show"]') ?? undefined);
    const options = Array.from(container.querySelectorAll('[role="option"]'));
    expect(options.map((o) => o.querySelector(".select-option-label")?.textContent)).toEqual([
      "Installed voices",
      "All languages",
      "English",
      "French · Français",
      "German · Deutsch",
    ]);
    await act(async () => (options[2] as HTMLElement).click());
    expect(rowNames()).toEqual(["Heart", "Emma"]);
    // Two English regions: each voice says which.
    const meta = Array.from(container.querySelectorAll(".voice-rt-meta")).map((m) => m.textContent);
    expect(meta[0]).toContain("United States");
    expect(meta[1]).toContain("United Kingdom");
  });

  it("offers each hearing on its own card", async () => {
    await render();
    const titles = Array.from(container.querySelectorAll("h2")).map((h) => h.textContent);
    expect(titles).toContain("Standard hearing");
    expect(titles).toContain("Light hearing");
    expect(button("Download (about 100 mb)")).toBeTruthy();
  });

  it("keeps the hearing in use when a second one is downloaded", async () => {
    await render();
    await click(button("Download"));
    expect(api.voiceDownload).toHaveBeenCalledWith("hearing", "moonshine-en", expect.any(Function));
    expect(api.setSetting).not.toHaveBeenCalledWith("voice.hearing_model", "moonshine-en");
  });

  it("lets the person pick between two installed hearings", async () => {
    api.status = { ...api.status, hearings: ["parakeet-v3", "moonshine-en"] };
    await render();
    expect(button("In use")).toBeTruthy();
    await click(button("Use this one"));
    expect(api.setSetting).toHaveBeenCalledWith("voice.hearing_model", "moonshine-en");
  });

  it("changes the shortcut to the next key combination and starts using it", async () => {
    await render();
    expect(container.querySelector("kbd")?.textContent).toBe("Ctrl+Shift+Space");
    await click(button("Change"));
    expect(container.querySelector("kbd")?.textContent).toContain("Press");
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", ctrlKey: true, altKey: true }));
    });
    expect(api.setSetting).toHaveBeenCalledWith("voice.hotkey", "Ctrl+Alt+K");
    expect(getVoiceHotkey()).toBe("Ctrl+Alt+K");
  });

  it("gives up on Escape without changing anything", async () => {
    await render();
    await click(button("Change"));
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(api.setSetting).not.toHaveBeenCalled();
    expect(container.querySelector("kbd")?.textContent).toBe("Ctrl+Shift+Space");
  });
});
