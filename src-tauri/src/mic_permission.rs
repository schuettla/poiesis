//! Answers WebView2's microphone question on the app's behalf.
//!
//! Voice mode and dictation call `getUserMedia`. WebView2 then shows its own
//! "Allow this app to use your microphone?" box, and without a handler it asks
//! again whenever its stored answer is lost (a fresh profile, a new origin in a
//! dev build). The user already chose to talk by pressing the mic or opening
//! voice mode, so the box adds nothing. This allows the microphone for the app's
//! own pages and nothing else: not the camera, not location, and not the sandboxed
//! artifact frames, which keep WebView2's default (`VXP-3`: the mic is still only
//! opened by our own capture code, and the UI shows it whenever it is).
//!
//! The Windows privacy switch ("Let desktop apps access your microphone") is the
//! operating system's and is not touched here.

use tauri::{AppHandle, Manager};
use webview2_com::{
    take_pwstr,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
        COREWEBVIEW2_PERMISSION_STATE_ALLOW,
    },
    PermissionRequestedEventHandler,
};

/// The pages that are the app itself: the packaged app (`tauri://localhost` or
/// `http(s)://tauri.localhost`) and the Vite dev server.
fn is_app_origin(uri: &str) -> bool {
    const ORIGINS: [&str; 4] = [
        "tauri://localhost",
        "http://tauri.localhost",
        "https://tauri.localhost",
        "http://localhost:1420",
    ];
    ORIGINS.iter().any(|origin| {
        uri.strip_prefix(origin)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '?', '#']))
    })
}

/// Allows the microphone for the main window's own pages. Failing to register is
/// harmless: WebView2 simply keeps asking.
pub fn allow_for_app(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else { return };
    let _ = window.with_webview(|webview| unsafe {
        let Ok(core) = webview.controller().CoreWebView2() else { return };
        let mut token = Default::default();
        let _ = core.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                args.PermissionKind(&mut kind)?;
                if kind != COREWEBVIEW2_PERMISSION_KIND_MICROPHONE {
                    return Ok(());
                }
                let mut uri = Default::default();
                args.Uri(&mut uri)?;
                if is_app_origin(&take_pwstr(uri)) {
                    args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
                }
                Ok(())
            })),
            &mut token,
        );
    });
}

#[cfg(test)]
mod tests {
    use super::is_app_origin;

    #[test]
    fn only_the_app_itself_is_trusted() {
        assert!(is_app_origin("http://tauri.localhost/"));
        assert!(is_app_origin("https://tauri.localhost/index.html"));
        assert!(is_app_origin("tauri://localhost"));
        assert!(is_app_origin("http://localhost:1420/#/chat"));
        // Lookalikes and everything else keep WebView2's own question.
        assert!(!is_app_origin("http://tauri.localhost.evil.com/"));
        assert!(!is_app_origin("http://localhost:14200/"));
        assert!(!is_app_origin("http://127.0.0.1:8080/artifact.html"));
        assert!(!is_app_origin("https://example.com/"));
        assert!(!is_app_origin(""));
    }
}
