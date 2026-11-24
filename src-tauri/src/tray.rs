//! REM-1: Poiesis keeps running when the window is closed, so a phone can still reach it.
//!
//! Closing the window hides it and leaves a tray icon. The icon's menu has Open and Quit; a left
//! click opens the window. The behaviour is on by default and can be switched off with the setting
//! `app.keep_running` = `off`. Quit in the tray always ends the process.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, Window, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

use crate::db::Db;
use crate::remote::Gateway;

/// Setting: `off` means closing the window quits, as before. Anything else keeps running.
pub const KEEP_RUNNING_KEY: &str = "app.keep_running";
/// Set once the person has been told that Poiesis stays in the tray.
const NOTICE_KEY: &str = "app.keep_running_notice";
/// Argument added to the autostart command, so a login start leaves the window closed.
pub const HIDDEN_ARG: &str = "--hidden";

/// The close button hides the window unless the person switched this off.
pub fn should_keep_running(setting: Option<&str>) -> bool {
    setting != Some("off")
}

/// The app stays alive after its last window is gone, but only when nobody asked to exit. A real
/// exit (the tray's Quit, an update) carries an exit code and is never held back.
pub fn should_prevent_exit(setting: Option<&str>, code: Option<i32>) -> bool {
    code.is_none() && should_keep_running(setting)
}

fn keep_running(app: &AppHandle) -> bool {
    let setting = app
        .try_state::<Db>()
        .and_then(|db| db.get_setting(KEEP_RUNNING_KEY).ok().flatten());
    should_keep_running(setting.as_deref())
}

/// The tray icon and its menu. Called once from `setup`.
pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Poiesis", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    // The same switch as Settings > Remote (REM-3). The gateway keeps the tick in step.
    let remote = CheckMenuItem::with_id(app, "remote", "Remote control", true, false, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&open, &remote, &PredefinedMenuItem::separator(app)?, &quit],
    )?;
    app.manage(RemoteItem(remote));
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Poiesis")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "remote" => {
                if let Some(gw) = app.try_state::<Gateway>() {
                    // A failed switch leaves the tick flipped by the click; put it back to the truth.
                    if gw.set_enabled(!gw.is_enabled()).is_err() {
                        sync_remote(app, gw.is_enabled());
                    }
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// The tray's "Remote control" tick.
struct RemoteItem(CheckMenuItem<tauri::Wry>);

/// Set the tray's tick to what the switch really is. A click flips the tick by itself, so this
/// also puts it back when the switch could not change.
pub fn sync_remote(app: &AppHandle, enabled: bool) {
    if let Some(item) = app.try_state::<RemoteItem>() {
        let _ = item.0.set_checked(enabled);
    }
}

/// Bring the main window back from the tray or from behind other windows.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// `.on_window_event`: the close button hides the main window instead of closing it.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        let app = window.app_handle();
        if keep_running(app) {
            api.prevent_close();
            let _ = window.hide();
            tell_once(app);
        }
    }
}

/// `.run` callback: the last window going away does not end the process. Returns true when it
/// held the exit back, so the caller does not stop the engines of an app that lives on.
pub fn on_run_event(app: &AppHandle, event: &RunEvent) -> bool {
    if let RunEvent::ExitRequested { code, api, .. } = event {
        let setting = app
            .try_state::<Db>()
            .and_then(|db| db.get_setting(KEEP_RUNNING_KEY).ok().flatten());
        if should_prevent_exit(setting.as_deref(), *code) {
            api.prevent_exit();
            return true;
        }
    }
    false
}

/// A login start (autostart passes `--hidden`) leaves the window in the tray.
pub fn hide_if_started_hidden(app: &AppHandle) {
    if std::env::args().any(|a| a == HIDDEN_ARG) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
    }
}

/// The first time the window is hidden, say so once, so the person does not think it crashed.
fn tell_once(app: &AppHandle) {
    use tauri_plugin_notification::NotificationExt;

    let Some(db) = app.try_state::<Db>() else { return };
    if db.get_setting(NOTICE_KEY).ok().flatten().is_some() {
        return;
    }
    let _ = db.set_setting(NOTICE_KEY, "1");
    let _ = app
        .notification()
        .builder()
        .title("Poiesis is still here")
        .body("I keep running here so you can reach me. Quit from the tray.")
        .show();
}

/// Whether Poiesis starts when the person signs in.
#[tauri::command]
pub fn autostart_status_cmd(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

/// Switch "Start with Windows" on or off.
#[tauri::command]
pub fn autostart_set_cmd(app: AppHandle, on: bool) -> Result<bool, String> {
    let launch = app.autolaunch();
    if on {
        launch.enable().map_err(|e| e.to_string())?;
    } else {
        launch.disable().map_err(|e| e.to_string())?;
    }
    launch.is_enabled().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_hides_the_window_unless_the_person_switched_it_off() {
        assert!(should_keep_running(None), "on by default");
        assert!(should_keep_running(Some("on")));
        assert!(should_keep_running(Some("garbage")));
        assert!(!should_keep_running(Some("off")));
    }

    #[test]
    fn only_an_exit_without_a_code_is_held_back() {
        assert!(should_prevent_exit(None, None), "last window gone");
        assert!(!should_prevent_exit(None, Some(0)), "Quit in the tray");
        assert!(!should_prevent_exit(None, Some(1)));
        assert!(!should_prevent_exit(Some("off"), None), "the switch is off");
    }
}
