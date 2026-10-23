//! Finding an agent app on this computer (`CLI-2`).
//!
//! A catalog plus a search, never a disk scan: for each binary the catalog
//! names, look in a short list of known install places and then along the PATH.
//! Nothing here runs anything. It only asks whether those exact files exist.
//!
//! The PATH is read **fresh from the registry**, not from this process: the
//! process PATH is frozen when Poiesis starts, so an agent installed while the
//! app is open would stay invisible until a restart.

use std::path::{Path, PathBuf};

/// Where to look. A value, so a test can point it at a temp folder.
#[derive(Debug, Clone, Default)]
pub struct SearchRoots {
    pub user_profile: Option<PathBuf>,
    pub app_data: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    /// Folders of the user's current PATH, in order.
    pub path_dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    /// An npm `.cmd` / `.bat` wrapper. Spawned through `cmd.exe` (`CLI-3`).
    pub shim: bool,
}

pub fn is_shim(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
}

impl SearchRoots {
    /// The real machine: the user's folders, and the PATH as it is now.
    pub fn from_system() -> Self {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        let mut path_dirs = fresh_path_dirs();
        // The app's own PATH last: it can only add, and a development run
        // started from a shell may rely on it.
        if let Some(p) = std::env::var_os("PATH") {
            for d in std::env::split_paths(&p) {
                if !path_dirs.contains(&d) {
                    path_dirs.push(d);
                }
            }
        }
        SearchRoots {
            user_profile: env("USERPROFILE").or_else(|| env("HOME")),
            app_data: env("APPDATA"),
            local_app_data: env("LOCALAPPDATA"),
            path_dirs,
        }
    }

    /// The folders searched before the PATH, with the file names to try in each.
    fn known_places(&self) -> Vec<(PathBuf, &'static [&'static str])> {
        let mut out: Vec<(PathBuf, &'static [&'static str])> = Vec::new();
        if let Some(p) = &self.user_profile {
            // Native installers.
            out.push((p.join(".local").join("bin"), &["exe"]));
        }
        if let Some(a) = &self.app_data {
            // npm global.
            out.push((a.join("npm"), &["cmd"]));
        }
        if let Some(l) = &self.local_app_data {
            // WinGet links.
            out.push((l.join("Microsoft").join("WinGet").join("Links"), &["exe"]));
        }
        if let Some(p) = &self.user_profile {
            // Scoop shims.
            out.push((p.join("scoop").join("shims"), &["exe"]));
        }
        out
    }
}

/// First hit wins: the program the user set, then the known places, then PATH.
pub fn locate(binaries: &[String], user_program: Option<&Path>, roots: &SearchRoots) -> Option<Found> {
    if let Some(p) = user_program {
        // The user's choice is final: if it is gone, say so rather than
        // quietly using something else.
        return p.is_file().then(|| Found { path: p.to_path_buf(), shim: is_shim(p) });
    }
    for name in binaries {
        for (dir, exts) in roots.known_places() {
            if let Some(f) = first_file(&dir, name, exts) {
                return Some(f);
            }
        }
        for dir in &roots.path_dirs {
            if let Some(f) = first_file(dir, name, &["exe", "cmd"]) {
                return Some(f);
            }
        }
        // Last: the copy the editor extension brings along. Anyone who uses
        // Claude Code inside VS Code has it, even with no install of their own.
        if name == "claude" {
            if let Some(f) = roots.user_profile.as_deref().and_then(editor_extension_claude) {
                return Some(f);
            }
        }
    }
    None
}

/// `claude.exe` inside the newest Claude Code extension of VS Code. One folder
/// listing per editor, no deeper search.
fn editor_extension_claude(user_profile: &Path) -> Option<Found> {
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    for editor in [".vscode", ".vscode-insiders"] {
        let Ok(entries) = std::fs::read_dir(user_profile.join(editor).join("extensions")) else { continue };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(rest) = file_name.to_str().and_then(|n| n.strip_prefix("anthropic.claude-code-")) else { continue };
            let exe = entry.path().join("resources").join("native-binary").join("claude.exe");
            if !exe.is_file() {
                continue;
            }
            // "2.1.301-win32-x64" -> [2, 1, 301]
            let version: Vec<u64> = rest
                .split('-')
                .next()
                .unwrap_or("")
                .split('.')
                .map(|p| p.parse().unwrap_or(0))
                .collect();
            if best.as_ref().is_none_or(|(v, _)| version > *v) {
                best = Some((version, exe));
            }
        }
    }
    best.map(|(_, path)| Found { path, shim: false })
}

fn first_file(dir: &Path, name: &str, exts: &[&str]) -> Option<Found> {
    exts.iter().find_map(|ext| {
        let p = dir.join(format!("{name}.{ext}"));
        p.is_file().then(|| Found { shim: is_shim(&p), path: p })
    })
}

/// The user's PATH as it is in the registry right now, variables expanded.
#[cfg(windows)]
fn fresh_path_dirs() -> Vec<PathBuf> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let mut dirs = Vec::new();
    for (root, key) in [
        (HKEY_CURRENT_USER, "Environment"),
        (HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment"),
    ] {
        if let Some(value) = registry_string(root, key, "Path") {
            for d in std::env::split_paths(&expand_vars(&value)) {
                if !d.as_os_str().is_empty() && !dirs.contains(&d) {
                    dirs.push(d);
                }
            }
        }
    }
    dirs
}

#[cfg(not(windows))]
fn fresh_path_dirs() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn registry_string(
    root: windows_sys::Win32::System::Registry::HKEY,
    subkey: &str,
    name: &str,
) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, RegQueryValueExW, KEY_READ};
    unsafe {
        let mut key = std::ptr::null_mut();
        if RegOpenKeyExW(root, wide(subkey).as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return None;
        }
        let wname = wide(name);
        let mut size: u32 = 0;
        let ok = RegQueryValueExW(key, wname.as_ptr(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), &mut size) == 0;
        let mut out = None;
        if ok && size > 0 {
            let mut buf = vec![0u16; (size as usize).div_ceil(2)];
            let mut size2 = size;
            if RegQueryValueExW(key, wname.as_ptr(), std::ptr::null(), std::ptr::null_mut(), buf.as_mut_ptr() as *mut u8, &mut size2) == 0 {
                let len = (size2 as usize / 2).min(buf.len());
                let end = buf[..len].iter().position(|&c| c == 0).unwrap_or(len);
                out = Some(String::from_utf16_lossy(&buf[..end]));
            }
        }
        RegCloseKey(key);
        out
    }
}

#[cfg(windows)]
fn expand_vars(s: &str) -> std::ffi::OsString {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
    unsafe {
        let src = wide(s);
        let needed = ExpandEnvironmentStringsW(src.as_ptr(), std::ptr::null_mut(), 0);
        if needed == 0 {
            return s.into();
        }
        let mut buf = vec![0u16; needed as usize];
        let n = ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), needed);
        if n == 0 || n > needed {
            return s.into();
        }
        std::ffi::OsString::from_wide(&buf[..(n as usize).saturating_sub(1)])
    }
}

/// The PATH to give a child process: the fresh one, so an agent that starts
/// `node` itself finds a Node installed after Poiesis started.
pub fn child_path() -> Option<std::ffi::OsString> {
    let dirs = SearchRoots::from_system().path_dirs;
    if dirs.is_empty() {
        return None;
    }
    std::env::join_paths(dirs).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"").unwrap();
    }

    fn roots(tmp: &Path) -> SearchRoots {
        SearchRoots {
            user_profile: Some(tmp.join("home")),
            app_data: Some(tmp.join("appdata")),
            local_app_data: Some(tmp.join("local")),
            path_dirs: vec![tmp.join("pathdir")],
        }
    }

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    /// `CLI-T-12`: each install place is found.
    #[test]
    fn each_install_place_is_found() {
        let tmp = tempfile::tempdir().unwrap();
        let r = roots(tmp.path());
        let cases = [
            ("home/.local/bin/claude.exe", false),
            ("appdata/npm/claude.cmd", true),
            ("local/Microsoft/WinGet/Links/claude.exe", false),
            ("home/scoop/shims/claude.exe", false),
            ("pathdir/claude.exe", false),
        ];
        for (rel, shim) in cases {
            let tmp2 = tempfile::tempdir().unwrap();
            let r2 = SearchRoots { path_dirs: vec![tmp2.path().join("pathdir")], ..roots(tmp2.path()) };
            touch(&tmp2.path().join(rel));
            let found = locate(&names(&["claude"]), None, &r2).unwrap_or_else(|| panic!("{rel}"));
            assert_eq!(found.path, tmp2.path().join(rel));
            assert_eq!(found.shim, shim, "{rel}");
        }
        assert!(locate(&names(&["claude"]), None, &r).is_none(), "nothing there, nothing found");
    }

    /// The VS Code extension's copy is found, the newest one wins, and a real
    /// install on the PATH still beats it.
    #[test]
    fn the_vs_code_extension_copy_is_found_last() {
        let tmp = tempfile::tempdir().unwrap();
        let r = roots(tmp.path());
        let ext = |v: &str| format!("home/.vscode/extensions/anthropic.claude-code-{v}-win32-x64/resources/native-binary/claude.exe");
        touch(&tmp.path().join(ext("2.1.9")));
        touch(&tmp.path().join(ext("2.1.301")));
        // A folder without the program inside is skipped.
        fs::create_dir_all(tmp.path().join("home/.vscode/extensions/anthropic.claude-code-9.9.9-win32-x64")).unwrap();
        let found = locate(&names(&["claude"]), None, &r).unwrap();
        assert_eq!(found.path, tmp.path().join(ext("2.1.301")));
        assert!(!found.shim);
        touch(&tmp.path().join("pathdir/claude.exe"));
        assert_eq!(locate(&names(&["claude"]), None, &r).unwrap().path, tmp.path().join("pathdir/claude.exe"));
        // Only claude gets this; codex does not.
        assert!(locate(&names(&["codex"]), None, &r).is_none());
    }

    #[test]
    fn the_program_the_user_set_beats_everything_and_is_not_replaced_when_gone() {
        let tmp = tempfile::tempdir().unwrap();
        let r = roots(tmp.path());
        touch(&tmp.path().join("home/.local/bin/claude.exe"));
        let mine = tmp.path().join("mine/claude.exe");
        touch(&mine);
        assert_eq!(locate(&names(&["claude"]), Some(&mine), &r).unwrap().path, mine);
        fs::remove_file(&mine).unwrap();
        assert!(locate(&names(&["claude"]), Some(&mine), &r).is_none(), "a gone choice is not swapped for another");
    }

    /// A binary added to the PATH after startup shows up on the next search.
    #[test]
    fn a_binary_that_appears_later_is_found_next_time() {
        let tmp = tempfile::tempdir().unwrap();
        let r = roots(tmp.path());
        assert!(locate(&names(&["codex"]), None, &r).is_none());
        touch(&tmp.path().join("pathdir/codex.cmd"));
        assert!(locate(&names(&["codex"]), None, &r).unwrap().shim);
    }

    /// Detection only checks the names the catalog gave it.
    #[test]
    fn a_binary_nobody_asked_for_is_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let r = roots(tmp.path());
        touch(&tmp.path().join("pathdir/evil.exe"));
        assert!(locate(&names(&["claude", "codex"]), None, &r).is_none());
    }
}
