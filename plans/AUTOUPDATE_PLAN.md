# Project Poiesis — Auto-Update Plan

**Poiesis should be able to become a newer version of itself, and say so.**
Today the app is a build frozen at whatever moment its installer was made.
This plan gives it a way to notice that a newer self exists, fetch it, verify
it really came from us, install it, and tell the user in its own voice what
changed — without ever nagging, and without the user hunting for a download
page.

> ID prefixes — **REL** release pipeline (GitHub side, no app code) ·
> **UPD** app wiring (Rust + config) · **UPD-UI** frontend.
>
> **Build order: REL → UPD → UPD-UI.** REL-1..REL-6 must be done *first* and
> in order — you cannot configure the app until the signing key exists, and
> you cannot test the app until one real release is published.
>
> **PRES-0 (first-person copy) from `POIESIS_PLAN.md` binds every `-UI-` task
> here.** §5 is the authoritative copy table; build each UI task with its copy
> from the start, not as a polish pass.
>
> **Settled decisions (2026-08-11):**
> - **Windows first.** `REL-5`'s matrix ships Windows only. macOS updates are
>   pointless without an Apple Developer ID (Gatekeeper blocks the installed
>   `.app` regardless of what Tauri verifies), and Linux adds three bundle
>   formats to test. Both are one matrix entry away when we want them — §7.
> - **Tagged releases, not every commit.** The app follows published GitHub
>   Releases, not `master`. A nightly channel is designed for but parked (§7).
> - **The user is always asked before installing.** Poiesis downloads nothing
>   until the user says yes. An agent that rewrites its own binary silently is
>   exactly the thing this product is not.

---

# Part I — What this actually is

The request was "pull the latest commit into the distributed app." That is
worth restating precisely, because the mechanism is different from the mental
model and the difference determines everything below.

A Poiesis install is not source code. It is a compiled Rust binary, a bundled
webview, a built JS bundle, and a native installer — produced by a toolchain
(Rust, MSVC, Node, NSIS) that users do not have and should not need. So
"pulling the latest commit" onto a user's machine is not possible in any form
worth shipping.

What *is* possible, and what every Tauri app in production does:

```
you push a git tag  →  GitHub Actions builds + signs an installer
                    →  publishes it to GitHub Releases, plus a small
                       manifest file (latest.json) describing the newest
                       version
                    →  the installed app reads that manifest, sees a higher
                       version, downloads the installer, verifies our
                       signature, and installs it
```

From the user's side this is indistinguishable from "the app keeps itself
current." From ours it is: **commits are the input, signed binaries are the
distribution unit, and the version number is the contract.**

Two properties matter and are non-negotiable:

**Signing is not optional.** Tauri's updater verifies every downloaded bundle
against a minisign public key baked into the app at build time. If the
signature doesn't match, it refuses to install. This is the only thing
standing between our users and anyone who can serve them a file — a
compromised GitHub token, a hijacked release asset, a proxy on a hotel
network. The private key never enters the repo; it lives in a GitHub Actions
secret and on your machine.

**This is separate from Windows code signing.** Tauri's minisign check
protects the *update path*. It does nothing about SmartScreen, which warns on
unsigned installers from unknown publishers. Updates will work fine unsigned —
Tauri verifies, installs, relaunches — but the first-run install and each
update's installer step may show a SmartScreen prompt until we buy an
Authenticode / EV certificate. Out of scope here; noted so it isn't a surprise.

---

# Part II — REL: the release pipeline, from zero

This part is all GitHub and shell. No app code changes. Do it in order.

Current state: remote `poiesis` → `https://github.com/schuettla/poiesis.git`,
no `.github/` directory, `tauri.conf.json` version `0.1.0`, `package.json`
version `0.1.0`.

### REL-0 — Confirm the repo is public and reachable

The updater fetches `latest.json` and the installer over plain HTTPS with no
credentials. If the repo is private, every asset URL 404s for users and the
updater silently reports "no update."

- Open `https://github.com/schuettla/poiesis` in a logged-out browser (or a
  private window). If you get a 404, the repo is private: **Settings →
  General → Danger Zone → Change visibility → Public.**
- **Acceptance:** the repo page loads while signed out.

> If you'd rather keep the source private, the release *assets* must still be
> public — which means a second public repo used only for releases, with the
> workflow pushing artifacts there. That's a real option but it doubles the
> setup; §7 sketches it. This plan assumes public.

### REL-1 — Generate the update signing keypair

On your machine, in the repo root:

```powershell
npm run tauri signer generate -- -w "$env:USERPROFILE\.tauri\poiesis.key"
```

It prompts for a password — **set one**, and put it in your password manager
alongside the key. It writes two files:

| File | What it is | Where it goes |
| --- | --- | --- |
| `~/.tauri/poiesis.key` | **private** — signs releases | GitHub secret + your password manager. Never in git. |
| `~/.tauri/poiesis.key.pub` | **public** — verifies releases | pasted into `tauri.conf.json` (UPD-2) |

- Back the private key up somewhere you will still have in two years. If you
  lose it, you cannot ship an update that existing installs will accept —
  every user has to reinstall by hand. This is the single highest-consequence
  artifact in the whole plan.
- **Acceptance:** both files exist; `~/.tauri/` is outside the repo (verify
  `git status` shows nothing new).

### REL-2 — Put the private key into GitHub Actions secrets

On github.com, in the repo: **Settings → Secrets and variables → Actions →
New repository secret.** Create two:

| Name | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | the entire **contents** of `~/.tauri/poiesis.key` (open it in an editor, copy everything — it's one long base64 blob, not a path) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | the password you set in REL-1 |

- If you set no password in REL-1, still create the secret with an empty
  value — the build reads it unconditionally.
- **Acceptance:** both names appear in the secrets list. (You can never read
  them back; that's expected.)

### REL-3 — Understand the release flow you're about to automate

Before writing YAML, the vocabulary, because it's the part that trips people
new to GitHub Releases:

- A **tag** is a permanent name for one commit — `v0.1.1`. You create it with
  git and push it. That push is what starts everything.
- A **release** is a GitHub page attached to a tag, with a title, notes, and
  **assets** (files). Our installer and `latest.json` are assets.
- A **draft** release is invisible to the public and its assets are not
  downloadable anonymously. Our workflow creates a draft so you can review it;
  **the updater cannot see it until you publish.** This is the #1 "why isn't
  it working" cause.
- A **pre-release** is published and public but excluded from
  `/releases/latest`. That's how a beta channel would work (§7).

### REL-4 — Version discipline

`tauri-action` reads the version from `src-tauri/tauri.conf.json` and
substitutes it into the tag name. The updater compares semver: it offers an
update only when the manifest's version is **greater than** the running app's.

**The versioning policy (settled 2026-08-11).** Shipped today is `0.1.0`.

| Bump | When | Sequence |
| --- | --- | --- |
| **patch** — `0.1.0 → 0.1.1 → 0.1.2 → …` | the normal case: fixes, refinements, small additions. This is what almost every release is. | the default |
| **minor** — `0.1.x → 0.2.0` | a **major new release** — a capability that changes what Poiesis is, or a batch of work you'd write an announcement for. Deliberate, not incidental. | rare |
| **major** — `0.x → 1.0.0` | reserved. Not on the roadmap yet. | — |

So the **first release is `v0.1.1`**, not `v0.2.0`, and the second is
`v0.1.2`. `0.2.0` is a decision you make, never something a release script
reaches by counting.

- Patch numbers do not roll over at 9 — `0.1.9 → 0.1.10` is correct semver and
  the updater compares it correctly (it parses numerically, not as a string).
- **Never reuse or move a tag.** If a release is bad, ship `0.1.(n+1)` with the
  fix. Users who already installed the bad one have no path back otherwise, and
  a moved tag means two different binaries claiming the same version.
- Every release, before tagging, bump **both**:
  - `src-tauri/tauri.conf.json` → `"version"` (the source of truth — this is
    what the tag, the installer filename, `latest.json` and the About screen
    all derive from)
  - `package.json` → `"version"` (kept in sync so npm and the bundle never
    disagree with the shell)

- **Acceptance:** `git tag` name, `tauri.conf.json` version and `package.json`
  version all agree for any given release; the version is strictly greater
  than the previous published release.

### REL-5 — The release workflow

Create `.github/workflows/release.yml`:

```yaml
name: release

on:
  push:
    tags:
      - 'v*'
  workflow_dispatch:      # lets you run it by hand from the Actions tab

jobs:
  build:
    permissions:
      contents: write     # required: the job creates a Release
    strategy:
      fail-fast: false
      matrix:
        include:
          - platform: 'windows-latest'
            args: ''
          # macOS / Linux: see §7 before enabling.
          # - platform: 'macos-latest'
          #   args: '--target aarch64-apple-darwin'
          # - platform: 'ubuntu-22.04'
          #   args: ''

    runs-on: ${{ matrix.platform }}
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-node@v4
        with:
          node-version: lts/*
          cache: 'npm'

      - uses: dtolnay/rust-toolchain@stable

      - uses: swatinem/rust-cache@v2
        with:
          workspaces: './src-tauri -> target'

      - name: install frontend dependencies
        run: npm ci

      - uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        with:
          tagName: v__VERSION__
          releaseName: 'Poiesis Agent v__VERSION__'
          releaseBody: 'See the assets below to download and install this version.'
          releaseDraft: true
          prerelease: false
          includeUpdaterJson: true
          updaterJsonPreferNsis: true
          args: ${{ matrix.args }}
```

Notes on the non-obvious lines:

- `permissions: contents: write` — without it the built-in `GITHUB_TOKEN`
  can't create the release and the job fails at the very last step.
- `includeUpdaterJson: true` generates and uploads `latest.json`. This is the
  file the app polls. Without it there is no update feed.
- `updaterJsonPreferNsis: true` — our bundle target is `"all"`, so Windows
  produces both an NSIS `.exe` and a WiX `.msi`. NSIS is the better updater
  vehicle (installs per-user, no UAC prompt, supports silent/passive modes).
  This tells the manifest to point at the `.exe`.
- `releaseDraft: true` — deliberate. You get a human checkpoint before
  anything reaches users. See REL-6.
- **Verify the action's major tag at implementation time.** `@v0` is the
  long-standing tag; check the [tauri-action README](https://github.com/tauri-apps/tauri-action)
  for the current one and pin to it rather than to a moving branch.

- **Acceptance:** the file is committed and appears under the repo's
  **Actions** tab as a workflow named `release`.

### REL-6 — Cut the first release (the dry run)

Do this *after* UPD-1..UPD-3, so the first published release already contains
a real updater manifest.

```powershell
# 1. bump both version fields to 0.1.1 (REL-4), commit
git add -A
git commit -m "release: v0.1.1"

# 2. tag and push — this is the trigger
git tag v0.1.1
git push poiesis master
git push poiesis v0.1.1
```

Then on github.com:

1. **Actions** tab → watch the `release` run. First run takes ~10–20 min
   (cold Rust build). Red X → open the failing step; the error is almost
   always a missing secret or a compile error.
2. **Releases** (right sidebar of the repo home, or `/releases`) → you'll see
   **`Poiesis Agent v0.1.1` marked `Draft`**.
3. Click it → **Edit** → confirm the assets list contains:
   - `Poiesis Agent_0.1.1_x64-setup.exe` (the installer)
   - `Poiesis Agent_0.1.1_x64-setup.exe.sig` (its signature)
   - `latest.json` (the manifest)
4. Write real release notes in the body — **this text is what the app shows
   the user** (UPD-UI-1 renders it). Write it for them, not for us.
5. **Publish release.**
6. Sanity-check the feed is live and anonymous:
   `https://github.com/schuettla/poiesis/releases/latest/download/latest.json`
   should return JSON in a logged-out browser.

- **Acceptance:** that URL returns a JSON body whose `version` is `0.1.1` and
  whose `platforms."windows-x86_64".url` points at the `-setup.exe`.

---

# Part III — UPD: wiring the app

### UPD-1 — Add the plugins

```powershell
npm run tauri add updater
npm install @tauri-apps/plugin-process
```

`tauri add updater` edits `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs` and
`src-tauri/capabilities/default.json` for you. Verify each, and add the
process plugin by hand (needed to relaunch after install):

**`src-tauri/Cargo.toml`** — desktop-gated, matching what `tauri add` writes:

```toml
[target.'cfg(any(target_os = "macos", windows, target_os = "linux"))'.dependencies]
tauri-plugin-updater = "2"
tauri-plugin-process = "2"
```

**`src-tauri/src/lib.rs`** — inside the existing `.setup(|app| { ... })`,
alongside the other plugin registrations:

```rust
#[cfg(desktop)]
{
    app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
    app.handle().plugin(tauri_plugin_process::init())?;
}
```

**`src-tauri/capabilities/default.json`** — add to `permissions`:

```json
"updater:default",
"process:allow-restart"
```

- **Acceptance:** `cargo build` succeeds; `npm run tauri dev` starts with no
  capability warnings in the console.

### UPD-2 — Configure the updater

Edit `src-tauri/tauri.conf.json`:

```jsonc
"bundle": {
  "active": true,
  "targets": "all",
  "createUpdaterArtifacts": true,        // ← add
  // ...icons, windows, unchanged
},
"plugins": {
  "updater": {
    "pubkey": "<paste the entire contents of ~/.tauri/poiesis.key.pub>",
    "endpoints": [
      "https://github.com/schuettla/poiesis/releases/latest/download/latest.json"
    ],
    "windows": { "installMode": "passive" }
  }
}
```

- `createUpdaterArtifacts` is what makes the build emit the `.sig` files. Miss
  it and `includeUpdaterJson` has nothing to describe.
- `pubkey` is the key **content**, not a path.
- `installMode: "passive"` shows a small progress window during install with
  no prompts — honest about what's happening without demanding clicks.
  `"quiet"` is fully invisible; `"basicUi"` is the full wizard.
- **CSP is not a concern here.** The updater's HTTP requests happen in Rust
  (reqwest), not in the webview, so the existing `connect-src` in
  `app.security.csp` does not need a GitHub entry. Don't widen it.
- **Acceptance:** `npm run tauri build` produces `.sig` files next to the
  installer in `src-tauri/target/release/bundle/nsis/`.

### UPD-3 — Shut the engine down before the installer runs

This one is specific to us and will bite if skipped. Poiesis supervises child
processes (`llama-server`, and the image engine). A Windows update replaces
files under the install directory and then relaunches; if `llama-server` is
still holding a model file or a port, the install can fail or the new process
can come up onto an occupied port.

In `src-tauri/src/lib.rs`, build the updater with an exit hook that performs
the same shutdown the app already does on window close:

```rust
tauri_plugin_updater::Builder::new()
    .on_before_exit(|| {
        // Same teardown as normal shutdown: stop llama-server and any
        // image-engine child before the installer touches the install dir.
        crate::runtime::manager::shutdown_blocking();
    })
    .build()
```

- Reuse the existing shutdown path in `src-tauri/src/runtime/manager.rs`
  rather than writing a second one; if the current teardown is only wired to a
  window event, extract it into a callable function first.
- **Acceptance:** with a model loaded, running an update leaves no orphaned
  `llama-server.exe` in Task Manager, and the relaunched app starts its engine
  normally.

### UPD-4 — Frontend API surface

Add to `src/lib/api.ts`, next to the existing `getAppVersion`:

```ts
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateInfo = { version: string; notes: string; date: string | null };

/** Resolves null when already current. Throws on network/parse failure. */
export async function checkForUpdate(): Promise<Update | null> { ... }

/** Downloads + installs, reporting bytes as they arrive. */
export async function installUpdate(
  update: Update,
  onProgress: (downloaded: number, total: number | null) => void,
): Promise<void> { ... }

export const restartApp = () => relaunch();
```

`installUpdate` wraps `update.downloadAndInstall(cb)` and translates the three
event shapes (`Started` carries `contentLength`, `Progress` carries
`chunkLength` per chunk — accumulate it yourself, `Finished` carries nothing)
into a single running `(downloaded, total)` callback, so the UI never has to
know the event protocol.

- All four must no-op safely under `inTauri() === false` (browser preview),
  matching how `getAppVersion` is guarded in `About.tsx`.
- **Acceptance:** `tsc` clean; browser preview shows no console errors.

### UPD-5 — Store state

Add to `AppState` in `src/lib/store.ts`:

```ts
type UpdateState =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "current"; checkedAt: number }
  | { phase: "available"; info: UpdateInfo }
  | { phase: "downloading"; info: UpdateInfo; downloaded: number; total: number | null }
  | { phase: "ready"; info: UpdateInfo }
  | { phase: "error"; message: string };

updateState: UpdateState;
updateAutoCheck: boolean;              // persisted setting "updates.autocheck"
updateToast: UpdateInfo | null;        // drives UPD-UI-2
checkForUpdates: (manual: boolean) => Promise<void>;
startUpdateInstall: () => Promise<void>;
setUpdateAutoCheck: (on: boolean) => Promise<void>;
dismissUpdateToast: () => void;
```

- The live `Update` handle from the plugin is **not** stored in zustand (it
  isn't serialisable and doesn't belong in UI state) — keep it in a
  module-level `let pendingUpdate: Update | null` in the store file, with
  `UpdateInfo` being the serialisable projection the UI renders.
- `checkForUpdates(manual)` — when `manual` is false (the startup check),
  errors resolve to `{ phase: "idle" }` and raise nothing. A background check
  that fails because the user is offline is not an event worth showing. When
  `manual` is true, errors surface as `{ phase: "error" }`.
- **Acceptance:** vitest covers the phase machine: check→current,
  check→available, available→downloading→ready, and silent-vs-loud error.

### UPD-6 — Startup check and the notified-version latch

In `App.tsx`'s existing boot effect, when `inTauri()` and `updateAutoCheck`:
fire `checkForUpdates(false)` **after** first paint — deliberately not
blocking startup. On `available`, raise `updateToast` only if the found
version differs from the persisted `updates.notified_version` setting, then
write that setting.

- One notification per version, ever. Poiesis mentions a new version once; if
  the user ignores it, the badge (UPD-UI-3) is the only remaining trace.
- **Acceptance:** restarting the app twice with an update available shows the
  toast once and the badge both times.

---

# Part IV — UPD-UI: what the user actually sees and feels

The whole feature is four surfaces, in descending loudness. None of them
interrupt work.

### UPD-UI-1 — The Updates block in About

**File:** `src/routes/About.tsx`, a new `<section className="setting-block">`
inserted **between** the "Poiesis Agent" block (line ~35) and "Third-party
licenses" (line ~45). This is the home of the feature — everything else points
here.

Reuses the existing class vocabulary from `src/routes/Settings.css`:
`.setting-block`, `.setting-title`, `.setting-help`, `.setting-readout`,
`.btn-primary`, `.btn-secondary`, `.toggle-line`. Two new classes only:
`.update-progress` (a 3px track + fill bar, styled like `.imggen-progress`
which already exists at `Settings.css:155` — reuse it if it fits rather than
adding a second bar) and `.update-notes`.

Layout, top to bottom:

```
Updates                                    ← .setting-title
I check GitHub for a newer version of      ← .setting-help
myself. I'll never install one without
asking you.

  ┌─────────────────────────────────────┐
  │  [ state region — see table below ] │
  └─────────────────────────────────────┘

☐ Check when I start                       ← .toggle-line, default on
```

The state region by phase — this is the exact spec:

| phase | readout | control(s) |
| --- | --- | --- |
| `idle` | `Version 0.1.1` | `Check for updates` (`.btn-secondary`) |
| `checking` | `Checking…` | button disabled |
| `current` | `Version 0.1.1 — I'm up to date.` <br> `Checked just now` (relative, `.setting-readout`) | `Check again` (`.btn-secondary`) |
| `available` | `Version 0.1.2 is available.` <br> release notes in `.update-notes` | `Download and install` (`.btn-primary`) · `Not now` (`.btn-secondary`, → `idle`) |
| `downloading` | `Downloading… 12.4 MB of 48.1 MB` + `.update-progress` bar | none (no cancel in v1 — §7) |
| `ready` | `Installed. I'll be version 0.1.2 once I restart.` | `Restart now` (`.btn-primary`) · `Later` (`.btn-secondary`) |
| `error` | `I couldn't check just now — {message}.` | `Try again` (`.btn-secondary`) |

- Release notes render as **plain text with preserved line breaks**
  (`white-space: pre-wrap`), not markdown. The body comes from a GitHub
  release you wrote by hand; a markdown renderer here is a dependency and an
  injection surface for no gain.
- Cap `.update-notes` at ~12rem with `overflow-y: auto` so a long changelog
  never pushes the buttons off-screen.
- `Later` on `ready` is real: the update is already staged and applies on the
  user's next normal restart. Say that, don't just dismiss.
- **Acceptance:** every phase reachable and correct in browser preview with a
  stubbed store; bar animates against real byte counts in a Tauri build.

### UPD-UI-2 — The discovery toast

**File:** `src/components/Memory/MemoryToast.tsx` — add an `UpdateToast`
branch to the existing priority chain in the `if (!toast)` block (line ~123),
below `HealToast`. That file is already the single toast host; do not add a
second toast system.

Same `.memory-toast` shell, same 6s `DWELL_MS` auto-dismiss, one action:

```
◆  There's a newer version of me — 0.1.2.      [ See what's new ]
```

- `See what's new` → `setView("about")` (the About tab already exists in
  `SettingsHub`'s `TABS`, so this lands correctly with no routing work).
- Fires at most once per version, ever (UPD-6's latch).
- Priority: **below** `healToast` and memory writes. A self-repair or a memory
  write is about *this conversation*; a version notice can wait 6 seconds.

**This toast is the feature.** It's the moment Poiesis tells you it can become
something newer, in the same voice and the same shell it uses to say it
learned something. Get this line right and the rest is plumbing.

- **Acceptance:** with `updateToast` set and no memory/heal toast pending, the
  toast renders, auto-dismisses, and the badge survives it.

### UPD-UI-3 — The badge

Two edits, both following patterns already in place:

1. **`src/routes/SettingsHub.tsx:52`** — extend `badgeFor`:
   ```ts
   (v === "about" && updateAvailable)
   ```
   with `const updateAvailable = useAppStore((s) =>
   s.updateState.phase === "available" || s.updateState.phase === "ready");`

2. **`src/components/Rail/Rail.tsx:214`** — fold into the existing cog badge:
   ```ts
   const settingsPending = soulPending || selfPending || consolidationPending || updateAvailable;
   ```
   The comment there already says "one badge covers whatever's waiting in any
   of them" — extend that comment to mention updates so the next reader knows.

- The badge is the only *persistent* signal. It says "something is waiting"
  and costs the user nothing to ignore. It clears when the update installs, or
  when they hit `Not now` (→ `idle`).
- **Acceptance:** badge appears on the rail cog and the About tab together,
  and clears on both paths.

### UPD-UI-4 — "I'm now on 0.1.2" (the arrival receipt)

After an update installs and the app relaunches, the user's very next
experience should confirm the thing happened. Without this, an auto-updater is
invisible-by-design in the bad way — the app just silently mutates.

On boot, compare `getAppVersion()` against the persisted setting
`updates.last_seen_version`. If the running version is higher, raise a
`ReceiptToast` (the existing no-undo shell at `MemoryToast.tsx:52`) and write
the new value:

```
I'm now version 0.1.2.                        [ What changed ]
```

- `What changed` → `setView("about")`, where the notes for the version just
  installed are still shown.
- On a first-ever install (no stored value), write the version and show
  nothing — that's not an update.
- **Acceptance:** simulate by lowering the stored setting and restarting; the
  receipt appears once, and not on the next start.

---

# Part V — Copy table (authoritative)

First person, present tense, no exclamation marks, no "New!", no version
numbers without context. Poiesis speaks about itself as a thing that changes.

| Where | Copy |
| --- | --- |
| Block title | `Updates` |
| Block help | `I check GitHub for a newer version of myself. I'll never install one without asking you.` |
| Toggle | `Check when I start` |
| Idle button | `Check for updates` |
| Checking | `Checking…` |
| Up to date | `Version {v} — I'm up to date.` |
| Last checked | `Checked {relative}` |
| Available | `Version {v} is available.` |
| Install button | `Download and install` |
| Decline button | `Not now` |
| Downloading | `Downloading… {done} of {total}` |
| Ready | `Installed. I'll be version {v} once I restart.` |
| Restart button | `Restart now` |
| Defer button | `Later` |
| Error | `I couldn't check just now — {reason}.` |
| Retry button | `Try again` |
| Discovery toast | `There's a newer version of me — {v}.` / action `See what's new` |
| Arrival toast | `I'm now version {v}.` / action `What changed` |

Error `{reason}` is mapped, never raw: network failure → `I couldn't reach
GitHub`; signature failure → `that download didn't verify`; anything else →
`something went wrong`. A raw Rust error string in this UI is a bug.

---

# Part VI — Verification

**Unit (vitest):** UPD-5's phase machine, including silent-vs-loud errors and
the byte accumulation in `installUpdate`'s progress translation.

**Component:** every UPD-UI-1 phase renders its row from the §5 table;
UPD-UI-2 respects the toast priority chain.

**Live smoke — the only test that proves the feature.** Needs two real builds:

1. Build and install `0.1.1` locally (`npm run tauri build`, run the NSIS
   installer). Confirm About shows `Version 0.1.1 — I'm up to date.`
2. Bump to `0.1.2`, tag, push, let REL-5 run, **publish the draft** (REL-6).
3. In the installed `0.1.1`: badge appears within seconds of a check; toast
   fires once; About shows the real notes you wrote on the release page.
4. `Download and install` → progress moves against real bytes → `Restart now`.
5. After relaunch: About reads `0.1.2`, the arrival receipt shows once, and
   Task Manager has no orphaned `llama-server.exe` (UPD-3).

**Negative test — do this once, it's the one that matters.** Edit `latest.json`'s
signature on a scratch release (or point `endpoints` at a hand-made manifest
with a bad `signature`) and confirm the app **refuses** and lands in `error`
with `that download didn't verify`. If this test passes silently instead, the
pubkey is wrong and every user is unprotected.

---

# Part VII — Parked, and known limits

- **A nightly / `master` channel.** Run the same workflow on push to a
  `nightly` branch with `prerelease: true`, publishing a second manifest at
  `/releases/download/nightly/latest.json`. Add a channel picker in UPD-UI-1
  that swaps the endpoint via the runtime `updater_builder().endpoints(...)`
  API. Deliberately deferred: auto-shipping every commit to a userbase needs a
  rollback story first.
- **macOS.** One matrix line plus `x86_64`/`aarch64` targets, but requires an
  Apple Developer ID for signing and notarisation — without it the updated
  `.app` is quarantined and won't launch. Don't enable until that's bought.
- **Linux.** AppImage, `.deb` and `.rpm` all self-update in Tauri 2, but each
  needs its own smoke test and the AppImage path has its own caveats.
- **Windows code signing (SmartScreen).** Separate from update signing; costs
  money; doesn't block this plan.
- **Cancelling a download.** No cancel button in v1. The plugin's install is a
  single await; adding cancellation means restructuring around an abort
  handle. Fine to skip for a ~50 MB download; revisit if the bundle grows.
- **Private source, public releases.** If REL-0 is refused: keep this repo
  private, create a public `poiesis-releases` repo, and have the workflow
  publish there with a PAT (`owner`/`repo` inputs on `tauri-action`).
  Endpoints then point at the public repo. Costs one more secret and a second
  repo to keep tidy.
- **Delta updates.** Tauri ships whole installers. A ~50 MB download per
  update is acceptable; if it stops being, that's a different plan.
