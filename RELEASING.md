# Releasing Poiesis

How a new version reaches people. The design and the reasons are in
[plans/AUTOUPDATE_PLAN.md](plans/AUTOUPDATE_PLAN.md); this is the routine.

A release is a git tag. Pushing `v0.1.n` makes GitHub Actions build the
Windows installer, sign it for the updater, and attach it to a **draft**
release together with `latest.json`. Installed copies of Poiesis read
`latest.json` from the latest **published** release, and offer the update —
they never install without asking.

## One-time setup

1. The repo is public (the updater downloads anonymously).
2. A signing keypair exists and the private half is in GitHub **Settings →
   Secrets and variables → Actions** as `TAURI_SIGNING_PRIVATE_KEY` (the
   key's *contents*, not a path) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
3. The public half is the `pubkey` in `src-tauri/tauri.conf.json`.

The private key is the one irreplaceable thing. Lose it and no installed copy
will accept another update — everyone reinstalls by hand. It lives in a
password manager and a second backup, never in git.

## Every release

1. **Green.** On `master`: `npx tsc --noEmit`, `npm test`, and
   `cargo test` in `src-tauri` (this includes the last-release database
   migration test).
2. **Bump the version** in all three places, to the same number:
   `src-tauri/tauri.conf.json`, `package.json`, `src-tauri/Cargo.toml`. Then
   `node scripts/check-version.mjs` must print one version three times.
   - patch (`0.1.1 → 0.1.2`) is the default;
   - minor (`0.1.x → 0.2.0`) only for a release you would announce;
   - the version must be strictly greater than the last published one;
   - never reuse or move a tag. A bad release is fixed by the next patch.
3. **Commit, tag, push both.**
   ```powershell
   git commit -am "release: v0.1.2"
   git tag v0.1.2
   git push poiesis master
   git push poiesis v0.1.2
   ```
4. **Wait for the `release` workflow** (Actions tab; ~10–20 min cold). Then
   open the draft under **Releases** and check it lists the `-setup.exe`, its
   `.sig`, and `latest.json`.
5. **Write the notes for the people using it.** The app shows this text,
   verbatim and as plain text, in Settings → About → Updates. No markdown, no
   commit log.
6. **Smoke the update path** on your own machine: install the *previous*
   release, then the draft's installer over it, and confirm your conversations,
   memories and settings are intact.
7. **Publish.** Until then the updater cannot see the release.
8. **Refresh the migration fixture** if the database schema changed in this
   release, so the next release is tested against what users now have:
   ```powershell
   cd src-tauri
   cargo test regenerate_release_fixture -- --ignored
   ```
   Commit `src-tauri/tests/fixtures/release-last.db`.

## Checking the feed

`https://github.com/schuettla/poiesis/releases/latest/download/latest.json`
must return JSON in a logged-out browser, with the new `version` and a
`platforms."windows-x86_64".url` pointing at the `-setup.exe`.

## Building an installer locally

For test builds on your own devices, use:

```powershell
npm run build:local
```

It builds a normal installer without the update signature
(`src-tauri/tauri.local.conf.json` turns `createUpdaterArtifacts` off), so no
key is needed. The app inside still has the updater, the public key and the
GitHub endpoint, so it can check for and install real releases.

`npm run build:release` signs its output (`createUpdaterArtifacts` is on in
`tauri.conf.json`), so it needs the key in the environment. Only do this to
produce a signed installer yourself; CI does it for releases:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content "$env:USERPROFILE\.tauri\poiesis.key" -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "…"
npm run build:release
```

## If something goes wrong

- **Workflow fails at the tag check:** the version files and the tag
  disagree. Fix the files, delete the tag *that was never published* and push
  it again. (A tag that was published is never reused.)
- **The app says "I couldn't reach GitHub":** the release is still a draft, or
  the repo went private.
- **The app says "that download didn't verify":** the `pubkey` in the installed
  build doesn't match the key the release was signed with. Do not publish; find
  out why first.
- **Windows shows "Windows protected your PC":** releases are not code-signed
  yet. *More info → Run anyway.* The updater's own signature check is separate
  and always on.
