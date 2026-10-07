//! Voice setup, settings, previews and dictation (VOC-4, VOC-5, VOC-6).
//!
//! Engine code lives in `runtime/voice.rs`; this file is the thin command
//! layer: it resolves settings and paths, then calls the manager. Nothing
//! here sends audio or text off the machine.

use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Serialize;
use tauri::ipc::{Channel, InvokeBody, Request, Response};
use tauri::{AppHandle, State};

use crate::db::Db;
use crate::runtime::download::DownloadProgress;
use crate::runtime::hardware::{detect_hardware, HardwareProfile};
use crate::runtime::voice::{wav_bytes, VoiceManager, SAMPLE_RATE};
use crate::runtime::voice_session::{SessionConfig, VoiceEvent, VoiceSession, VoiceSessions};
use crate::runtime::voice_catalog::{
    default_choice, default_threads, install_archive, install_vad, voice_catalog, VoiceCatalog,
    VoicePaths,
};
use crate::runtime::RuntimeManager;
use crate::PoiesisError;

type Cmd<T> = Result<T, PoiesisError>;

fn err<E: std::fmt::Display>(e: E) -> PoiesisError {
    PoiesisError::Message(e.to_string())
}

/// Longest stretch of speech dictation accepts in one go.
const MAX_DICTATION_SECS: usize = 120;

/// Detecting hardware probes the OS, so do it once.
async fn hardware() -> Cmd<&'static HardwareProfile> {
    static HW: OnceLock<HardwareProfile> = OnceLock::new();
    if HW.get().is_none() {
        let profile = tauri::async_runtime::spawn_blocking(detect_hardware).await.map_err(err)?;
        let _ = HW.set(profile);
    }
    Ok(HW.get().expect("just set"))
}

fn paths(mgr: &RuntimeManager) -> VoicePaths {
    VoicePaths::new(mgr.app_data_dir())
}

/// The settings voice code runs on, with defaults filled in (VOC-5).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VoiceSettings {
    pub hearing_model: String,
    pub voice_id: String,
    /// 0.8 to 1.3.
    pub speed: f32,
    /// `auto` or a language code.
    pub language: String,
    /// False means push-to-talk only: speaking over Poiesis does not stop it.
    pub cut_in: bool,
    pub hotkey: String,
    pub threads: i32,
}

pub const KEY_HEARING: &str = "voice.hearing_model";
pub const KEY_VOICE: &str = "voice.voice_id";
pub const KEY_SPEED: &str = "voice.speed";
pub const KEY_LANGUAGE: &str = "voice.language";
pub const KEY_CUT_IN: &str = "voice.cut_in";
pub const KEY_HOTKEY: &str = "voice.hotkey";

/// Pure part of loading settings: `read` gives the stored value for a key.
pub fn resolve_settings(
    read: impl Fn(&str) -> Option<String>,
    hw: &HardwareProfile,
    ui_language: &str,
) -> VoiceSettings {
    let language = read(KEY_LANGUAGE)
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "auto".into());
    let basis = if language == "auto" { ui_language } else { language.as_str() };
    let (hearing, voice_model) = default_choice(hw, basis);
    let default_voice = voice_catalog()
        .voices
        .into_iter()
        .find(|m| m.id == voice_model)
        .and_then(|m| m.voices.into_iter().next())
        .map(|v| v.id)
        .expect("default voice model is in the catalog");
    VoiceSettings {
        hearing_model: read(KEY_HEARING).filter(|v| !v.is_empty()).unwrap_or_else(|| hearing.into()),
        voice_id: read(KEY_VOICE).filter(|v| !v.is_empty()).unwrap_or(default_voice),
        speed: read(KEY_SPEED)
            .and_then(|v| v.parse::<f32>().ok())
            .map(|v| v.clamp(0.8, 1.3))
            .unwrap_or(1.0),
        language,
        cut_in: read(KEY_CUT_IN).map(|v| v != "off").unwrap_or(true),
        hotkey: read(KEY_HOTKEY).filter(|v| !v.is_empty()).unwrap_or_else(|| "Ctrl+Shift+Space".into()),
        threads: default_threads(hw),
    }
}

async fn load_settings(db: &Db, ui_language: &str) -> Cmd<VoiceSettings> {
    let hw = hardware().await?;
    Ok(resolve_settings(|k| db.get_setting(k).ok().flatten(), hw, ui_language))
}

/// The hearing to use: the chosen one when it is installed, else any installed
/// one, so a person who only has one is never stuck. With none installed it
/// returns the chosen id, and the engine says what to download.
fn hearing_to_use(paths: &VoicePaths, chosen: &str) -> String {
    let status = status_for(paths, false);
    if status.hearings.iter().any(|h| h == chosen) {
        return chosen.to_string();
    }
    status.hearing.unwrap_or_else(|| chosen.to_string())
}

#[tauri::command]
pub async fn voice_settings_cmd(db: State<'_, Db>, ui_language: String) -> Cmd<VoiceSettings> {
    load_settings(&db, &ui_language).await
}

#[tauri::command]
pub fn voice_catalog_cmd() -> VoiceCatalog {
    voice_catalog()
}

/// What is installed and whether an engine is in memory (VOC-4).
#[derive(Debug, Serialize)]
pub struct VoiceStatus {
    /// Id of an installed hearing, if hearing is ready to use.
    pub hearing: Option<String>,
    /// Ids of every installed hearing (more than one can be installed).
    pub hearings: Vec<String>,
    /// Ids of installed voice models.
    pub voices: Vec<String>,
    pub loaded: bool,
}

fn status_for(paths: &VoicePaths, loaded: bool) -> VoiceStatus {
    let catalog = voice_catalog();
    // Hearing needs its model and the small speech-detection file.
    let hearings: Vec<String> = if paths.vad_file().exists() {
        catalog.hearing.iter().filter(|h| paths.is_installed(&h.id)).map(|h| h.id.clone()).collect()
    } else {
        Vec::new()
    };
    let hearing = hearings.first().cloned();
    let voices = catalog.voices.iter().filter(|v| paths.is_installed(&v.id)).map(|v| v.id.clone()).collect();
    VoiceStatus { hearing, hearings, voices, loaded }
}

#[tauri::command]
pub fn voice_status_cmd(mgr: State<'_, RuntimeManager>, voice: State<'_, VoiceManager>) -> VoiceStatus {
    status_for(&paths(&mgr), voice.loaded())
}

/// `kind` is `"hearing"` or `"voice"`; `id` is a catalog id.
#[tauri::command]
pub async fn voice_download_cmd(
    mgr: State<'_, RuntimeManager>,
    voice: State<'_, VoiceManager>,
    kind: String,
    id: String,
    on_progress: Channel<DownloadProgress>,
) -> Cmd<VoiceStatus> {
    let paths = paths(&mgr);
    let catalog = voice_catalog();
    let send = |p: DownloadProgress| {
        let _ = on_progress.send(p);
    };
    match kind.as_str() {
        "hearing" => {
            let entry = catalog.hearing.iter().find(|h| h.id == id).ok_or_else(|| err("unknown hearing"))?;
            if !paths.vad_file().exists() {
                install_vad(&mgr.client, &paths, send).await.map_err(err)?;
            }
            // Only the small detection file may be missing: do not fetch 490 MB again for it.
            if !paths.is_installed(&entry.id) {
                install_archive(&mgr.client, &paths, &entry.id, &entry.archive, "Getting Poiesis ready to listen", send)
                    .await
                    .map_err(err)?;
            }
        }
        "voice" => {
            let entry = catalog.voices.iter().find(|v| v.id == id).ok_or_else(|| err("unknown voice"))?;
            install_archive(&mgr.client, &paths, &entry.id, &entry.archive, "Getting a voice ready", send)
                .await
                .map_err(err)?;
        }
        _ => return Err(err("unknown kind")),
    }
    Ok(status_for(&paths, voice.loaded()))
}

#[tauri::command]
pub async fn voice_delete_cmd(
    mgr: State<'_, RuntimeManager>,
    voice: State<'_, VoiceManager>,
    kind: String,
    id: String,
) -> Cmd<VoiceStatus> {
    let paths = paths(&mgr);
    let catalog = voice_catalog();
    let known = match kind.as_str() {
        "hearing" => catalog.hearing.iter().any(|h| h.id == id),
        "voice" => catalog.voices.iter().any(|v| v.id == id),
        _ => false,
    };
    if !known {
        return Err(err("unknown item"));
    }
    // Windows keeps loaded model files open, so let go of them first.
    voice.unload().await;
    let dir: PathBuf = paths.model_dir(&id);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(err(e)),
    }
    // The speech-detection file goes with the last hearing.
    if kind == "hearing" && !catalog.hearing.iter().any(|h| paths.is_installed(&h.id)) {
        let _ = std::fs::remove_file(paths.vad_file());
    }
    Ok(status_for(&paths, voice.loaded()))
}

/// A short spoken sample for the settings page. Returns a WAV file.
#[tauri::command]
pub async fn voice_preview_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    voice: State<'_, VoiceManager>,
    voice_id: String,
    text: String,
) -> Cmd<Response> {
    let settings = load_settings(&db, "en").await?;
    let (samples, rate) = voice
        .speak(&paths(&mgr), &voice_id, settings.threads, text, settings.speed)
        .await
        .map_err(err)?;
    Ok(Response::new(wav_bytes(&samples, rate)))
}

/// Dictation: the body is one stretch of speech as 16 kHz mono 16-bit
/// little-endian samples. Returns what was said.
#[tauri::command]
pub async fn voice_transcribe_cmd(
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    voice: State<'_, VoiceManager>,
    request: Request<'_>,
) -> Cmd<String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err(err("expected raw audio"));
    };
    let samples = pcm16_to_f32(bytes, MAX_DICTATION_SECS * SAMPLE_RATE as usize)?;
    if samples.is_empty() {
        return Ok(String::new());
    }
    // The body is the audio, so the interface language comes as a header.
    let ui_language = request
        .headers()
        .get("x-ui-language")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("en")
        .to_string();
    let settings = load_settings(&db, &ui_language).await?;
    let paths = paths(&mgr);
    let hearing = hearing_to_use(&paths, &settings.hearing_model);
    voice.hear(&paths, &hearing, settings.threads, samples).await.map_err(err)
}

/// Longest batch of audio one push may carry. The screen sends about 100 ms.
const MAX_PUSH_SECS: usize = 2;

/// Opens a live voice session for the conversation. Closes any earlier one.
/// Fails with one plain sentence when hearing is not installed (VXP-7).
#[tauri::command]
pub async fn voice_start_cmd(
    app: AppHandle,
    db: State<'_, Db>,
    mgr: State<'_, RuntimeManager>,
    sessions: State<'_, VoiceSessions>,
    ui_language: String,
    on_event: Channel<VoiceEvent>,
) -> Cmd<()> {
    let settings = load_settings(&db, &ui_language).await?;
    let paths = paths(&mgr);
    if status_for(&paths, false).hearing.is_none() {
        return Err(err("Poiesis needs to download its hearing before it can listen."));
    }
    let config = SessionConfig {
        hearing_id: hearing_to_use(&paths, &settings.hearing_model),
        voice_id: settings.voice_id,
        speed: settings.speed,
        language: settings.language,
        cut_in: settings.cut_in,
        threads: settings.threads,
    };
    let session = VoiceSession::start(app, on_event, config, paths).map_err(err)?;
    sessions.replace(Some(session));
    Ok(())
}

#[tauri::command]
pub fn voice_stop_cmd(sessions: State<'_, VoiceSessions>) {
    sessions.replace(None);
}

/// One batch of mic audio: an 8-byte little-endian counter, then 16 kHz mono
/// 16-bit little-endian samples (AUD-2).
#[tauri::command]
pub fn voice_push_audio_cmd(sessions: State<'_, VoiceSessions>, request: Request<'_>) -> Cmd<()> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err(err("expected raw audio"));
    };
    if bytes.len() < 8 {
        return Err(err("audio batch is too short"));
    }
    let counter = u64::from_le_bytes(bytes[..8].try_into().expect("8 bytes"));
    let samples = pcm16_to_f32(&bytes[8..], MAX_PUSH_SECS * SAMPLE_RATE as usize)?;
    if let Some(session) = sessions.get() {
        session.push_audio(counter, &samples);
    }
    Ok(())
}

/// `state` is `"started"` (Poiesis began to make sound) or `"finished"` (it
/// stopped, for any reason).
#[tauri::command]
pub fn voice_assistant_cmd(sessions: State<'_, VoiceSessions>, state: String, generation: u64) -> Cmd<()> {
    let started = match state.as_str() {
        "started" => true,
        "finished" => false,
        _ => return Err(err("unknown state")),
    };
    if let Some(session) = sessions.get() {
        session.assistant(started, generation);
    }
    Ok(())
}

/// Streamed reply text to speak. `done` says no more text follows for this
/// reply. A `notice` is a short spoken line that is not part of the reply.
#[tauri::command]
pub fn voice_speak_cmd(
    sessions: State<'_, VoiceSessions>,
    generation: u64,
    text: String,
    done: bool,
    notice: Option<bool>,
    language: Option<String>,
) {
    if let Some(session) = sessions.get() {
        session.speak(generation, &text, done, notice.unwrap_or(false), language);
    }
}

/// Stops speaking reply `generation` and anything older.
#[tauri::command]
pub fn voice_cancel_speech_cmd(sessions: State<'_, VoiceSessions>, generation: u64) {
    if let Some(session) = sessions.get() {
        session.cancel_speech(generation);
    }
}

/// Raw 16-bit little-endian samples to floats in -1.0..1.0. Refuses more than
/// `max_samples` so one call cannot hold the engine for minutes.
pub fn pcm16_to_f32(bytes: &[u8], max_samples: usize) -> Cmd<Vec<f32>> {
    if !bytes.len().is_multiple_of(2) {
        return Err(err("audio has an odd number of bytes"));
    }
    if bytes.len() / 2 > max_samples {
        return Err(err("That was too long to type out in one go. Try a shorter stretch."));
    }
    Ok(bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::hardware::CpuInfo;
    use std::collections::HashMap;

    fn hw(ram_mb: u64) -> HardwareProfile {
        HardwareProfile {
            cpu: CpuInfo { brand: "t".into(), physical_cores: 8, avx2: true, avx512: false },
            ram_mb,
            gpus: vec![],
        }
    }

    fn settings(pairs: &[(&str, &str)], ram_mb: u64, ui: &str) -> VoiceSettings {
        let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        resolve_settings(|k| map.get(k).cloned(), &hw(ram_mb), ui)
    }

    #[test]
    fn defaults_when_nothing_is_stored() {
        let s = settings(&[], 16 * 1024, "en-US");
        assert_eq!(s.hearing_model, "parakeet-v3");
        assert_eq!(s.voice_id, "af_heart");
        assert_eq!(s.speed, 1.0);
        assert_eq!(s.language, "auto");
        assert!(s.cut_in);
        assert_eq!(s.hotkey, "Ctrl+Shift+Space");
        assert_eq!(s.threads, 4);
    }

    #[test]
    fn a_german_interface_gets_the_german_voice() {
        assert_eq!(settings(&[], 16 * 1024, "de-DE").voice_id, "piper-de-thorsten");
        assert_eq!(settings(&[(KEY_LANGUAGE, "de")], 16 * 1024, "en-US").voice_id, "piper-de-thorsten");
    }

    #[test]
    fn stored_values_win_and_speed_is_clamped() {
        let s = settings(&[(KEY_VOICE, "am_michael"), (KEY_SPEED, "9"), (KEY_CUT_IN, "off")], 16 * 1024, "en");
        assert_eq!(s.voice_id, "am_michael");
        assert_eq!(s.speed, 1.3);
        assert!(!s.cut_in);
        assert_eq!(settings(&[(KEY_SPEED, "0.1")], 16 * 1024, "en").speed, 0.8);
        assert_eq!(settings(&[(KEY_SPEED, "oops")], 16 * 1024, "en").speed, 1.0);
    }

    #[test]
    fn pcm_converts_and_is_bounded() {
        let v = pcm16_to_f32(&[0x00, 0x40, 0x00, 0xC0], 10).unwrap();
        assert_eq!(v, vec![0.5, -0.5]);
        assert!(pcm16_to_f32(&[1, 2, 3], 10).is_err());
        assert!(pcm16_to_f32(&[0; 8], 3).is_err());
    }

    #[test]
    fn status_needs_the_detection_file_for_hearing() {
        let tmp = tempfile::tempdir().unwrap();
        let p = VoicePaths::new(tmp.path());
        std::fs::create_dir_all(p.model_dir("parakeet-v3")).unwrap();
        std::fs::write(p.model_dir("parakeet-v3").join(".installed"), b"ok").unwrap();
        assert_eq!(status_for(&p, false).hearing, None);
        std::fs::write(p.vad_file(), b"x").unwrap();
        assert_eq!(status_for(&p, false).hearing.as_deref(), Some("parakeet-v3"));
    }

    #[test]
    fn the_chosen_hearing_is_used_when_installed_else_any_installed_one() {
        let tmp = tempfile::tempdir().unwrap();
        let p = VoicePaths::new(tmp.path());
        std::fs::write(p.vad_file(), b"x").unwrap_or_else(|_| {
            std::fs::create_dir_all(tmp.path().join("voice")).unwrap();
            std::fs::write(p.vad_file(), b"x").unwrap();
        });
        let install = |id: &str| {
            std::fs::create_dir_all(p.model_dir(id)).unwrap();
            std::fs::write(p.model_dir(id).join(".installed"), b"ok").unwrap();
        };
        // Nothing installed: the chosen id comes back, so the engine can say what to get.
        assert_eq!(hearing_to_use(&p, "parakeet-v3"), "parakeet-v3");
        install("moonshine-en");
        assert_eq!(hearing_to_use(&p, "parakeet-v3"), "moonshine-en");
        install("parakeet-v3");
        assert_eq!(hearing_to_use(&p, "parakeet-v3"), "parakeet-v3");
        assert_eq!(hearing_to_use(&p, "moonshine-en"), "moonshine-en");
        assert_eq!(status_for(&p, false).hearings.len(), 2);
    }
}
