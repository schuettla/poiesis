//! Poiesis on the phone (REM-14): a thin shell around the link to the desktop. The page calls five
//! commands; the key, the address and the connection stay here.

mod client;

use client::{Link, State};
use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{Emitter, Manager};
use tokio::sync::oneshot;

/// The page listens to this for `connecting`, `connected` and `notfound`.
const LINK_EVENT: &str = "remote-link";

struct App {
    link: Link,
    /// The one long-lived `subscribe` stream. Dropping this sender ends it, so a new feed replaces the old.
    feed: std::sync::Mutex<Option<oneshot::Sender<()>>>,
}

#[tauri::command]
async fn remote_state_cmd(app: tauri::State<'_, App>) -> Result<State, String> {
    Ok(app.link.state().await)
}

/// Pair with the desktop whose pairing text this is. Returns the desktop's name.
#[tauri::command]
async fn remote_pair_cmd(app: tauri::State<'_, App>, text: String, label: String) -> Result<String, String> {
    app.link.pair(&text, &label).await
}

/// Make sure there is a live connection (on start and when the app comes back to the front).
#[tauri::command]
async fn remote_connect_cmd(app: tauri::State<'_, App>) -> Result<State, String> {
    let _ = app.link.connection().await;
    Ok(app.link.state().await)
}

#[tauri::command]
async fn remote_call_cmd(app: tauri::State<'_, App>, cmd: String, args: Option<Value>) -> Result<Value, String> {
    app.link.call(&cmd, args.unwrap_or(Value::Null)).await
}

/// A command that answers with events. They arrive on `on_event` until it is done.
#[tauri::command]
async fn remote_stream_cmd(
    app: tauri::State<'_, App>,
    cmd: String,
    args: Option<Value>,
    on_event: Channel<Value>,
) -> Result<(), String> {
    let is_feed = cmd == "subscribe";
    let work = app.link.stream(&cmd, args.unwrap_or(Value::Null), |event| {
        let _ = on_event.send(event);
    });
    if !is_feed {
        return work.await;
    }
    // One feed at a time: the old one ends when the page asks for a new one.
    let (replaced, stop) = oneshot::channel::<()>();
    *app.feed.lock().unwrap() = Some(replaced);
    tokio::select! {
        result = work => result,
        _ = stop => Ok(()),
    }
}

/// The live voice stream. Events arrive on `on_event` until the session ends.
#[tauri::command]
async fn remote_voice_open_cmd(
    app: tauri::State<'_, App>,
    args: Option<Value>,
    on_event: Channel<Value>,
) -> Result<(), String> {
    app.link
        .voice_open(args.unwrap_or(Value::Null), |event| {
            let _ = on_event.send(event);
        })
        .await
}

/// One batch of microphone audio. It comes as a list of bytes in JSON, not as a raw body: on Android,
/// Tauri sends every call through `postMessage`, which turns a raw body into JSON anyway.
#[tauri::command]
async fn remote_voice_push_cmd(app: tauri::State<'_, App>, audio: Vec<u8>) -> Result<(), String> {
    app.link.voice_push(&audio).await
}

#[tauri::command]
async fn remote_voice_close_cmd(app: tauri::State<'_, App>) -> Result<(), String> {
    app.link.voice_close().await;
    Ok(())
}

#[tauri::command]
async fn remote_forget_cmd(app: tauri::State<'_, App>) -> Result<(), String> {
    app.feed.lock().unwrap().take();
    app.link.forget().await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    builder
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let handle = app.handle().clone();
            let link = Link::open(
                dir,
                false,
                Box::new(move |state| {
                    let _ = handle.emit(LINK_EVENT, state);
                }),
            )?;
            app.manage(App { link, feed: std::sync::Mutex::new(None) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            remote_state_cmd,
            remote_pair_cmd,
            remote_connect_cmd,
            remote_call_cmd,
            remote_stream_cmd,
            remote_voice_open_cmd,
            remote_voice_push_cmd,
            remote_voice_close_cmd,
            remote_forget_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("Poiesis could not start");
}
