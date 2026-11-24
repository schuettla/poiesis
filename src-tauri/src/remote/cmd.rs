//! REM-3: the commands the desktop window uses to run the Remote tab. They are not on the phone's
//! allow-list: remote cannot change remote (plan, Part III.8).

use serde::Serialize;
use tauri::State;

use super::pairing::{self, Sender};
use super::telegram::{self, Status as TelegramStatus, Telegram, TelegramApi};
use super::Gateway;
use crate::db::Db;

#[derive(Serialize)]
pub struct RemoteStatus {
    pub enabled: bool,
    pub connected: bool,
    pub senders: Vec<Sender>,
}

fn status(gw: &Gateway, db: &Db) -> RemoteStatus {
    RemoteStatus { enabled: gw.is_enabled(), connected: gw.is_connected(), senders: pairing::load(db) }
}

#[tauri::command]
pub fn remote_status_cmd(gw: State<'_, Gateway>, db: State<'_, Db>) -> RemoteStatus {
    status(&gw, &db)
}

#[tauri::command]
pub fn remote_set_enabled_cmd(gw: State<'_, Gateway>, db: State<'_, Db>, on: bool) -> Result<RemoteStatus, String> {
    gw.set_enabled(on)?;
    Ok(status(&gw, &db))
}

/// What the Pair dialog shows: the code to type, the same as a QR image, and the same as text to copy.
#[derive(Serialize, Debug)]
pub struct PairingView {
    pub code: String,
    pub expires_at: i64,
    /// An SVG string of the QR code.
    pub svg: String,
    /// The text the QR holds, for a phone that cannot scan.
    pub text: String,
}

/// The QR code of `text` as SVG. Black on white with a quiet border, whatever the app theme, so
/// every camera reads it.
pub fn qr_svg(text: &str) -> Result<String, String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(text.as_bytes()).map_err(|e| format!("I could not draw the code. {e}"))?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(220, 220)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build())
}

/// What the phone needs to find this desktop, with the code. `None` while the link has no address yet.
pub fn pairing_payload(addr: Option<super::HostAddr>, code: &str) -> Option<remote_proto::Payload> {
    let addr = addr.filter(|a| !a.id.is_empty())?;
    Some(remote_proto::Payload { v: remote_proto::Payload::VERSION, id: addr.id, relay: addr.relay, code: code.to_string() })
}

/// A one-time code for the Pair dialog, with the QR the phone scans. Needs remote to be on.
#[tauri::command]
pub async fn remote_new_code_cmd(gw: State<'_, Gateway>) -> Result<PairingView, String> {
    let code = gw.new_code()?;
    // The relay is known a moment after the link starts. Wait for it, but not for long.
    let addr = gw.reachable(std::time::Duration::from_secs(6)).await;
    let payload = pairing_payload(addr, &code.code).ok_or("Remote is still starting. Try again in a moment.")?;
    let text = payload.to_text();
    Ok(PairingView { code: code.code, expires_at: code.expires_at, svg: qr_svg(&text)?, text })
}

/// The pairing dialog closed: its code stops working at once, so no stranger may knock any more.
#[tauri::command]
pub fn remote_cancel_code_cmd(gw: State<'_, Gateway>) {
    gw.cancel_code();
}

/// What the Telegram block shows: the bot, the last error in Telegram's words, whether it is listening.
#[tauri::command]
pub fn remote_telegram_status_cmd(tg: State<'_, Telegram>) -> TelegramStatus {
    tg.status()
}

/// Check the token with `getMe`, then keep it in the credential store. The error is Telegram's own words.
#[tauri::command]
pub async fn remote_telegram_connect_cmd(
    gw: State<'_, Gateway>,
    tg: State<'_, Telegram>,
    token: String,
) -> Result<TelegramStatus, String> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("Paste the token BotFather gave you.".into());
    }
    let me = telegram::HttpApi::new(&token).get_me().await.map_err(|e| e.text)?;
    crate::secrets::set_secret(crate::secrets::SERVICE_REMOTE, telegram::TOKEN_ACCOUNT, &token)
        .map_err(|e| format!("I could not keep the token. {e}"))?;
    tg.set_connected(&token, &me.name());
    tg.token_changed();
    gw.status_changed();
    Ok(tg.status())
}

/// Forget the token. Paired Telegram accounts stay; the bot stops listening.
#[tauri::command]
pub fn remote_telegram_disconnect_cmd(gw: State<'_, Gateway>, tg: State<'_, Telegram>) -> Result<TelegramStatus, String> {
    crate::secrets::delete_secret(crate::secrets::SERVICE_REMOTE, telegram::TOKEN_ACCOUNT).map_err(|e| e.to_string())?;
    tg.set_disconnected();
    tg.token_changed();
    gw.status_changed();
    Ok(tg.status())
}

/// A one-time code for "Pair an account": the person sends it to the bot. Needs remote to be on.
#[tauri::command]
pub fn remote_telegram_code_cmd(gw: State<'_, Gateway>) -> Result<super::pairing::PairingCode, String> {
    gw.new_code()
}

#[tauri::command]
pub fn remote_senders_cmd(db: State<'_, Db>) -> Vec<Sender> {
    pairing::load(&db)
}

/// Remove a paired sender. It has to pair again with a new code.
#[tauri::command]
pub fn remote_remove_sender_cmd(
    gw: State<'_, Gateway>,
    db: State<'_, Db>,
    transport: String,
    id: String,
) -> Result<bool, String> {
    let removed = pairing::remove(&db, &transport, &id)?;
    // A phone that is removed loses its open link at once, not at its next request.
    gw.kick(&id);
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::HostAddr;

    #[test]
    fn the_qr_is_an_svg_of_the_payload_text() {
        let payload = pairing_payload(Some(HostAddr { id: "ab".repeat(32), relay: Some("https://r.example./".into()) }), "K7M2QX9P")
            .unwrap();
        let svg = qr_svg(&payload.to_text()).unwrap();
        assert!(svg.starts_with("<?xml") || svg.contains("<svg"), "{}", &svg[..40.min(svg.len())]);
        assert!(svg.contains("#ffffff") && svg.contains("#000000"), "fixed colors, readable on any theme");
    }

    #[test]
    fn no_address_no_payload() {
        assert!(pairing_payload(None, "K7M2QX9P").is_none());
        assert!(pairing_payload(Some(HostAddr::default()), "K7M2QX9P").is_none());
    }

    #[test]
    fn the_payload_carries_the_relay_and_the_code() {
        let p = pairing_payload(Some(HostAddr { id: "k".into(), relay: None }), "AAAAAAAA").unwrap();
        assert_eq!(p.code, "AAAAAAAA");
        assert_eq!(p.relay, None);
        assert_eq!(remote_proto::Payload::parse(&p.to_text()), Some(p));
    }
}
