//! Voice engine (VOC): in-process speech detection, hearing and speaking on
//! top of sherpa-onnx. CPU only, so it never competes with the chat model for
//! VRAM. See plans/VOICE_PLAN.md.
//!
//! This file holds the model configs. Each builder takes the folder a model
//! was unpacked into and returns a sherpa config, so the catalog (VOC-2), the
//! manager (VOC-1) and the spike test (VOC-0.3) all load models one way.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use sherpa_onnx::{
    GenerationConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig,
    OfflineMoonshineModelConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKokoroModelConfig, OfflineTtsVitsModelConfig,
    SileroVadModelConfig, VadModelConfig,
};
use tauri::Manager;
use tokio::sync::Mutex;

use super::voice_catalog::{voice_catalog, HearingFamily, VoiceChoice, VoiceFamily, VoicePaths};

/// Idle time with no use before engines are dropped to free RAM (VOC-1).
pub const IDLE_UNLOAD: Duration = Duration::from_secs(5 * 60);

/// Speech detection runs on 16 kHz audio, 32 ms (512 sample) windows.
pub const SAMPLE_RATE: i32 = 16_000;

fn path_str(dir: &Path, name: &str) -> Option<String> {
    Some(dir.join(name).to_string_lossy().into_owned())
}

/// Silero voice detection. sherpa only reports a yes/no `detected()`, not the
/// raw probability, so the turn machine gets 0.0 or 1.0 (see Build notes).
pub fn vad_config(model: &Path, num_threads: i32) -> VadModelConfig {
    VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model.to_string_lossy().into_owned()),
            threshold: 0.5,
            // Short on purpose: the turn machine owns the pause timing (TRN-3),
            // so this must not add its own quarter second on top.
            min_silence_duration: 0.1,
            min_speech_duration: 0.1,
            window_size: 512,
            max_speech_duration: 30.0,
        },
        sample_rate: SAMPLE_RATE,
        num_threads,
        provider: Some("cpu".into()),
        debug: false,
        ..Default::default()
    }
}

/// Parakeet TDT 0.6B v3 (int8), a NeMo transducer.
pub fn parakeet_config(dir: &Path, num_threads: i32) -> OfflineRecognizerConfig {
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.transducer = OfflineTransducerModelConfig {
        encoder: path_str(dir, "encoder.int8.onnx"),
        decoder: path_str(dir, "decoder.int8.onnx"),
        joiner: path_str(dir, "joiner.int8.onnx"),
    };
    config.model_config.tokens = path_str(dir, "tokens.txt");
    config.model_config.model_type = Some("nemo_transducer".into());
    config.model_config.num_threads = num_threads;
    config.model_config.provider = Some("cpu".into());
    config
}

/// Moonshine v2 (English, quantized): two files, an encoder and a merged decoder.
pub fn moonshine_config(dir: &Path, num_threads: i32) -> OfflineRecognizerConfig {
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.moonshine = OfflineMoonshineModelConfig {
        encoder: path_str(dir, "encoder_model.ort"),
        merged_decoder: path_str(dir, "decoder_model_merged.ort"),
        ..Default::default()
    };
    config.model_config.tokens = path_str(dir, "tokens.txt");
    config.model_config.num_threads = num_threads;
    config.model_config.provider = Some("cpu".into());
    config
}

/// Kokoro multi-language v1.0, English lexicon only. `model` is the `.onnx`
/// file name inside `dir` (`model.int8.onnx` or `model.onnx`).
pub fn kokoro_config(dir: &Path, espeak: &Path, model: &str, num_threads: i32) -> OfflineTtsConfig {
    let mut config = OfflineTtsConfig::default();
    config.model.kokoro = OfflineTtsKokoroModelConfig {
        model: path_str(dir, model),
        voices: path_str(dir, "voices.bin"),
        tokens: path_str(dir, "tokens.txt"),
        data_dir: Some(espeak.to_string_lossy().into_owned()),
        lexicon: path_str(dir, "lexicon-us-en.txt"),
        ..Default::default()
    };
    config.model.num_threads = num_threads;
    config.model.provider = Some("cpu".into());
    config
}

/// A Piper voice (VITS). `model` is the `.onnx` file name inside `dir`.
pub fn piper_config(dir: &Path, espeak: &Path, model: &str, num_threads: i32) -> OfflineTtsConfig {
    let mut config = OfflineTtsConfig::default();
    config.model.vits = OfflineTtsVitsModelConfig {
        model: path_str(dir, model),
        tokens: path_str(dir, "tokens.txt"),
        data_dir: Some(espeak.to_string_lossy().into_owned()),
        ..Default::default()
    };
    config.model.num_threads = num_threads;
    config.model.provider = Some("cpu".into());
    config
}

#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    /// User-facing (VXP-7): says what to get, in plain words.
    #[error("{0}")]
    NotInstalled(&'static str),
    #[error("The voice could not be started: {0}")]
    Load(String),
    #[error("The voice stopped unexpectedly: {0}")]
    Run(String),
}

/// 16-bit little-endian PCM from float samples, clamped.
pub fn f32_to_pcm16(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// A mono 16-bit WAV file, so the WebView can play it with `decodeAudioData`
/// and the sample rate travels with the bytes.
pub fn wav_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let pcm = f32_to_pcm16(samples);
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(&pcm);
    out
}

/// Loads speech engines on first use and drops them after [`IDLE_UNLOAD`].
/// Engines are CPU only and run on blocking threads.
pub struct VoiceManager {
    hearing: Mutex<Option<(String, Arc<OfflineRecognizer>)>>,
    /// One decode at a time: a live session may ask for a partial while the
    /// last turn is still being heard, and two at once would only fight for CPU.
    decode: Mutex<()>,
    voice: Mutex<Option<(String, Arc<OfflineTts>)>>,
    last_used: StdMutex<Instant>,
    loaded: AtomicBool,
}

impl Default for VoiceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceManager {
    pub fn new() -> Self {
        Self {
            hearing: Mutex::new(None),
            decode: Mutex::new(()),
            voice: Mutex::new(None),
            last_used: StdMutex::new(Instant::now()),
            loaded: AtomicBool::new(false),
        }
    }

    /// Counts as use, so idle unload waits. A live voice session calls this.
    pub fn touch(&self) {
        *self.last_used.lock().unwrap() = Instant::now();
    }

    pub fn idle_for(&self) -> Duration {
        self.last_used.lock().unwrap().elapsed()
    }

    /// True while any engine is in memory.
    pub fn loaded(&self) -> bool {
        self.loaded.load(Ordering::Relaxed)
    }

    pub async fn unload(&self) {
        *self.hearing.lock().await = None;
        *self.voice.lock().await = None;
        self.loaded.store(false, Ordering::Relaxed);
    }

    /// Speech to text for one finished stretch of speech (16 kHz mono floats).
    pub async fn hear(
        &self,
        paths: &VoicePaths,
        hearing_id: &str,
        threads: i32,
        samples: Vec<f32>,
    ) -> Result<String, VoiceError> {
        let recognizer = self.ensure_hearing(paths, hearing_id, threads).await?;
        let _one_at_a_time = self.decode.lock().await;
        self.touch();
        let text = tokio::task::spawn_blocking(move || {
            let stream = recognizer.create_stream();
            stream.accept_waveform(SAMPLE_RATE, &samples);
            recognizer.decode(&stream);
            stream.get_result().map(|r| r.text.trim().to_string()).unwrap_or_default()
        })
        .await
        .map_err(|e| VoiceError::Run(e.to_string()))?;
        self.touch();
        Ok(text)
    }

    /// Loads hearing ahead of the first turn, so it does not wait for it.
    pub async fn warm_hearing(&self, paths: &VoicePaths, hearing_id: &str, threads: i32) -> Result<(), VoiceError> {
        self.ensure_hearing(paths, hearing_id, threads).await.map(|_| ())
    }

    async fn ensure_hearing(
        &self,
        paths: &VoicePaths,
        hearing_id: &str,
        threads: i32,
    ) -> Result<Arc<OfflineRecognizer>, VoiceError> {
        let mut slot = self.hearing.lock().await;
        if let Some((id, engine)) = slot.as_ref() {
            if id == hearing_id {
                return Ok(engine.clone());
            }
        }
        let family = voice_catalog().hearing.iter().find(|h| h.id == hearing_id).map(|h| h.family);
        let Some(family) = family.filter(|_| paths.is_installed(hearing_id)) else {
            return Err(VoiceError::NotInstalled(
                "Poiesis needs to download its hearing before it can listen.",
            ));
        };
        let dir = paths.model_dir(hearing_id);
        let config = match family {
            HearingFamily::Parakeet => parakeet_config(&dir, threads),
            HearingFamily::Moonshine => moonshine_config(&dir, threads),
        };
        let engine = tokio::task::spawn_blocking(move || OfflineRecognizer::create(&config))
            .await
            .map_err(|e| VoiceError::Load(e.to_string()))?
            .ok_or_else(|| VoiceError::Load("the hearing files could not be opened".into()))?;
        let engine = Arc::new(engine);
        *slot = Some((hearing_id.to_string(), engine.clone()));
        self.loaded.store(true, Ordering::Relaxed);
        Ok(engine)
    }

    /// Finds the catalog voice and loads its model (shared by every voice in
    /// that model). Returns the engine and the chosen voice.
    pub async fn ensure_voice(
        &self,
        paths: &VoicePaths,
        voice_id: &str,
        threads: i32,
    ) -> Result<(Arc<OfflineTts>, VoiceChoice), VoiceError> {
        let (model, choice) = voice_catalog()
            .voices
            .into_iter()
            .find_map(|m| {
                let c = m.voices.iter().find(|v| v.id == voice_id).cloned()?;
                Some((m, c))
            })
            .ok_or_else(|| VoiceError::Load(format!("unknown voice {voice_id}")))?;
        let mut slot = self.voice.lock().await;
        if let Some((id, engine)) = slot.as_ref() {
            if *id == model.id {
                return Ok((engine.clone(), choice));
            }
        }
        if !paths.is_installed(&model.id) {
            return Err(VoiceError::NotInstalled(
                "Poiesis needs to download a voice before it can speak.",
            ));
        }
        let (paths, id, family, file) = (paths.clone(), model.id.clone(), model.family, model.model_file.clone());
        let engine = tokio::task::spawn_blocking(move || {
            // The first voice may copy the shared speech data (about 18 MB): disk work.
            let espeak = paths.espeak_for(&id);
            let dir = paths.model_dir(&id);
            let config = match family {
                VoiceFamily::Kokoro => kokoro_config(&dir, &espeak, &file, threads),
                VoiceFamily::Piper => piper_config(&dir, &espeak, &file, threads),
            };
            OfflineTts::create(&config)
        })
        .await
        .map_err(|e| VoiceError::Load(e.to_string()))?
        .ok_or_else(|| VoiceError::Load("the voice files could not be opened".into()))?;
        let engine = Arc::new(engine);
        *slot = Some((model.id.clone(), engine.clone()));
        self.loaded.store(true, Ordering::Relaxed);
        Ok((engine, choice))
    }

    /// Speak one piece of text and return all of it as float samples plus the
    /// sample rate. `speed` is 0.8 to 1.3.
    pub async fn speak(
        &self,
        paths: &VoicePaths,
        voice_id: &str,
        threads: i32,
        text: String,
        speed: f32,
    ) -> Result<(Vec<f32>, u32), VoiceError> {
        let (engine, choice) = self.ensure_voice(paths, voice_id, threads).await?;
        self.touch();
        let out = tokio::task::spawn_blocking(move || {
            let config = GenerationConfig { speed, sid: choice.sid, ..Default::default() };
            engine
                .generate_with_config(&text, &config, None::<fn(&[f32], f32) -> bool>)
                .map(|a| (a.samples().to_vec(), a.sample_rate() as u32))
        })
        .await
        .map_err(|e| VoiceError::Run(e.to_string()))?
        .ok_or_else(|| VoiceError::Run("the voice produced no sound".into()))?;
        self.touch();
        Ok(out)
    }
}

/// Background loop: drops idle engines every 30 s (same shape as the recall
/// engine idle stop).
pub fn spawn_idle_unload(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let Some(mgr) = app.try_state::<VoiceManager>() else {
                return;
            };
            if mgr.loaded() && mgr.idle_for() >= IDLE_UNLOAD {
                mgr.unload().await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use sherpa_onnx::{
        GenerationConfig, LinearResampler, OfflineRecognizer, OfflineTts, VoiceActivityDetector, Wave,
    };
    use std::path::PathBuf;
    use std::time::Instant;

    /// VOC-0.2: proves the static library links and the C API answers. Empty
    /// configs have no model files, so both constructors must return `None`
    /// instead of crashing.
    #[test]
    fn engine_links_and_rejects_empty_config() {
        assert!(OfflineTts::create(&OfflineTtsConfig::default()).is_none());
        assert!(OfflineRecognizer::create(&OfflineRecognizerConfig::default()).is_none());
    }

    #[test]
    fn config_builders_point_at_the_expected_files() {
        let dir = Path::new("models/x");
        let asr = parakeet_config(dir, 4);
        assert!(asr.model_config.transducer.joiner.unwrap().ends_with("joiner.int8.onnx"));
        assert_eq!(asr.model_config.model_type.as_deref(), Some("nemo_transducer"));
        let moon = moonshine_config(dir, 2);
        assert!(moon.model_config.moonshine.merged_decoder.unwrap().ends_with("decoder_model_merged.ort"));
        assert!(moon.model_config.moonshine.encoder.unwrap().ends_with("encoder_model.ort"));
        let vad = vad_config(Path::new("silero_vad.onnx"), 1);
        assert_eq!(vad.silero_vad.window_size, 512);
        assert_eq!(vad.sample_rate, SAMPLE_RATE);
    }

    #[test]
    fn pcm16_clamps_and_is_little_endian() {
        assert_eq!(f32_to_pcm16(&[0.0, 1.0, -1.0, 2.0, -2.0]), vec![0, 0, 0xFF, 0x7F, 0x01, 0x80, 0xFF, 0x7F, 0x01, 0x80]);
    }

    #[test]
    fn wav_has_a_valid_header_for_mono_16_bit() {
        let wav = wav_bytes(&[0.0, 0.5, -0.5], 24_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 24_000);
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1); // mono
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 6);
        assert_eq!(wav.len(), 44 + 6);
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()) as usize, wav.len() - 8);
    }

    #[tokio::test]
    async fn missing_models_say_what_to_get() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = VoicePaths::new(tmp.path());
        let mgr = VoiceManager::new();
        let heard = mgr.hear(&paths, "parakeet-v3", 1, vec![0.0; 1600]).await.unwrap_err();
        assert!(heard.to_string().contains("download its hearing"), "{heard}");
        let spoke = mgr.speak(&paths, "piper-de-thorsten", 1, "Hallo".into(), 1.0).await.unwrap_err();
        assert!(spoke.to_string().contains("download a voice"), "{spoke}");
        assert!(!mgr.loaded());
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.path().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    /// End to end with real files: install the German voice and the speech
    /// detection file from the network, speak a sentence, hear it back. Needs
    /// `POIESIS_VOICE_MODELS` (for the hearing files) and internet.
    #[tokio::test]
    #[ignore]
    async fn install_speak_and_hear_it_back() {
        use super::super::voice_catalog::{install_archive, install_vad};
        let models = PathBuf::from(std::env::var("POIESIS_VOICE_MODELS").expect("set POIESIS_VOICE_MODELS"));
        let tmp = tempfile::tempdir().unwrap();
        let paths = VoicePaths::new(tmp.path());
        let client = reqwest::Client::new();

        install_vad(&client, &paths, |_| {}).await.unwrap();
        let piper = voice_catalog().voices.into_iter().find(|v| v.id == "piper-de-thorsten").unwrap();
        install_archive(&client, &paths, &piper.id, &piper.archive, "test", |_| {}).await.unwrap();
        assert!(paths.is_installed("piper-de-thorsten") && paths.vad_file().exists());

        copy_dir(&models.join("parakeet"), &paths.model_dir("parakeet-v3"));
        std::fs::write(paths.model_dir("parakeet-v3").join(".installed"), b"ok").unwrap();

        let mgr = VoiceManager::new();
        let (samples, rate) = mgr
            .speak(&paths, "piper-de-thorsten", 4, "Heute ist das Wetter schön und ich gehe spazieren.".into(), 1.0)
            .await
            .unwrap();
        assert!(mgr.loaded());
        let at_16k = sherpa_onnx::LinearResampler::create(rate as i32, SAMPLE_RATE)
            .unwrap()
            .resample(&samples, true);
        let text = mgr.hear(&paths, "parakeet-v3", 4, at_16k).await.unwrap();
        println!("heard back: {text:?}");
        assert!(text.to_lowercase().contains("wetter"), "{text}");
        mgr.unload().await;
        assert!(!mgr.loaded());
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/voice").join(name)
    }

    /// Every Piper voice in the catalog unpacks, loads and speaks. Needs
    /// `POIESIS_VOICE_DOWNLOADS` to hold the catalog's archives (by file name).
    #[test]
    #[ignore]
    fn every_listed_voice_speaks() {
        use super::super::voice_catalog::{unpack_tar_bz2, VoiceFamily};
        let dir = PathBuf::from(std::env::var("POIESIS_VOICE_DOWNLOADS").expect("set POIESIS_VOICE_DOWNLOADS"));
        let mut done = 0;
        // One speech-data folder for the whole run, as the app does (see `VoicePaths::espeak_for`).
        let work = tempfile::tempdir().unwrap();
        let shared = work.path().join("espeak-ng-data");
        let hearing = std::env::var("POIESIS_VOICE_MODELS")
            .ok()
            .and_then(|m| OfflineRecognizer::create(&parakeet_config(&PathBuf::from(m).join("parakeet"), 4)));
        for model in voice_catalog().voices.iter().filter(|m| m.family == VoiceFamily::Piper) {
            let archive = dir.join(&model.archive.file_name);
            if !archive.exists() {
                println!("skipped {} (not downloaded)", model.id);
                continue;
            }
            let tmp = tempfile::tempdir().unwrap();
            unpack_tar_bz2(&archive, tmp.path()).unwrap();
            let inner = std::fs::read_dir(tmp.path()).unwrap().next().unwrap().unwrap().path();
            if !shared.exists() {
                copy_dir(&inner.join("espeak-ng-data"), &shared);
            }
            let tts = OfflineTts::create(&piper_config(&inner, &shared, &model.model_file, 4))
                .unwrap_or_else(|| panic!("{} loads", model.id));
            for voice in &model.voices {
                let text = match voice.language.as_str() {
                    "ru" => "Привет, как дела сегодня? Погода очень хорошая.",
                    "el" => "Γεια σου, τι κάνεις σήμερα; Ο καιρός είναι πολύ καλός.",
                    "de" => "Hallo, wie geht es dir heute? Das Wetter ist sehr schön.",
                    "fr" => "Bonjour, comment vas-tu aujourd'hui ? Il fait très beau.",
                    "es" => "Hola, cómo estás hoy. El tiempo es muy bueno.",
                    "it" => "Ciao, come stai oggi? Il tempo è molto bello.",
                    "nl" => "Hallo, hoe gaat het vandaag? Het weer is erg mooi.",
                    "pt" => "Olá, como você está hoje? O tempo está muito bom.",
                    "pl" => "Cześć, jak się dzisiaj masz? Pogoda jest bardzo ładna.",
                    "sv" => "Hej, hur mår du idag? Vädret är mycket fint.",
                    "da" => "Hej, hvordan har du det i dag? Vejret er meget flot.",
                    "fi" => "Hei, mitä kuuluu tänään? Sää on erittäin hyvä.",
                    "cs" => "Ahoj, jak se dnes máš? Počasí je velmi pěkné.",
                    "hu" => "Szia, hogy vagy ma? Az idő nagyon szép.",
                    "ro" => "Salut, ce mai faci azi? Vremea este foarte frumoasă.",
                    _ => "Hello, how are you today? The weather is very nice.",
                };
                let audio = tts
                    .generate_with_config(
                        text,
                        &GenerationConfig { sid: voice.sid, ..Default::default() },
                        None::<fn(&[f32], f32) -> bool>,
                    )
                    .unwrap_or_else(|| panic!("{} speaks", voice.id));
                let secs = audio.samples().len() as f32 / audio.sample_rate() as f32;
                // Hear it back when the hearing files are at hand (a check by ear, not an assert).
                let heard = hearing.as_ref().map(|asr| {
                    let at_16k = LinearResampler::create(audio.sample_rate(), SAMPLE_RATE)
                        .unwrap()
                        .resample(audio.samples(), true);
                    let stream = asr.create_stream();
                    stream.accept_waveform(SAMPLE_RATE, &at_16k);
                    asr.decode(&stream);
                    stream.get_result().map(|r| r.text).unwrap_or_default()
                });
                println!("{} / {}: {secs:.1}s heard {:?}", model.id, voice.name, heard);
                assert!((0.6..12.0).contains(&secs), "{}: {secs}s", voice.id);
                done += 1;
            }
        }
        println!("spoke with {done} voices");
        assert!(done > 0);
    }

    /// The light hearing (Moonshine, English) understands the English fixture.
    /// Needs `POIESIS_VOICE_MODELS` with a `moonshine/` folder (the unpacked archive).
    #[test]
    #[ignore]
    fn light_hearing_understands_english() {
        let root = PathBuf::from(std::env::var("POIESIS_VOICE_MODELS").expect("set POIESIS_VOICE_MODELS"));
        let t = Instant::now();
        let asr = OfflineRecognizer::create(&moonshine_config(&root.join("moonshine"), 4)).expect("moonshine loads");
        println!("moonshine load: {:?}", t.elapsed());
        let wave = Wave::read(fixture("en_short.wav").to_str().unwrap()).expect("wav reads");
        let samples = LinearResampler::create(wave.sample_rate(), SAMPLE_RATE)
            .expect("resampler")
            .resample(wave.samples(), true);
        let t = Instant::now();
        let stream = asr.create_stream();
        stream.accept_waveform(SAMPLE_RATE, &samples);
        asr.decode(&stream);
        let text = stream.get_result().expect("result").text;
        println!("moonshine {:?} -> {text:?}", t.elapsed());
        assert!(!text.trim().is_empty());
    }

    /// VOC-0.3 and VOC-0.5. Needs the models on disk, so it is ignored by
    /// default. Point `POIESIS_VOICE_MODELS` at a folder holding:
    /// `silero_vad.onnx`, `parakeet/`, `kokoro/`, `piper-thorsten/` (each the
    /// unpacked model folder's contents). Run with:
    /// `cargo test --lib voice::tests::spike -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn spike_models_load_and_time() {
        let root = PathBuf::from(
            std::env::var("POIESIS_VOICE_MODELS").expect("set POIESIS_VOICE_MODELS"),
        );
        let threads: i32 = std::env::var("VOICE_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
        println!("threads: {threads}");

        // Voice detection over the English fixture: it must find speech.
        let t = Instant::now();
        let vad = VoiceActivityDetector::create(&vad_config(&root.join("silero_vad.onnx"), 1), 30.0)
            .expect("silero loads");
        println!("vad load: {:?}", t.elapsed());

        // Hearing.
        let t = Instant::now();
        let asr = OfflineRecognizer::create(&parakeet_config(&root.join("parakeet"), threads))
            .expect("parakeet loads");
        println!("parakeet load: {:?}", t.elapsed());

        for name in ["en_short.wav", "de_short.wav"] {
            let wave = Wave::read(fixture(name).to_str().unwrap()).expect("wav reads");
            let secs = wave.num_samples() as f32 / wave.sample_rate() as f32;
            // The app feeds 16 kHz from the capture worklet; the fixtures are 24 kHz.
            let samples = LinearResampler::create(wave.sample_rate(), SAMPLE_RATE)
                .expect("resampler")
                .resample(wave.samples(), true);

            vad.reset();
            for window in samples.chunks(512) {
                vad.accept_waveform(window);
            }
            vad.flush();
            let mut segments = 0;
            while !vad.is_empty() {
                segments += 1;
                vad.pop();
            }
            println!("{name}: {secs:.1}s, vad segments: {segments}");
            assert!(segments >= 1, "vad found no speech in {name}");

            let t = Instant::now();
            let stream = asr.create_stream();
            stream.accept_waveform(SAMPLE_RATE, &samples);
            asr.decode(&stream);
            let text = stream.get_result().expect("result").text;
            println!("{name}: parakeet {:?} -> {text:?}", t.elapsed());
            assert!(!text.trim().is_empty());
        }

        // Speaking: English with Kokoro, German with Piper Thorsten.
        let speak = |label: &str, tts: &OfflineTts, text: &str| {
            let started = Instant::now();
            let cb_first = std::sync::Arc::new(std::sync::Mutex::new(None::<std::time::Duration>));
            let cb = cb_first.clone();
            let audio = tts
                .generate_with_config(
                    text,
                    &GenerationConfig::default(),
                    Some(move |_: &[f32], _: f32| {
                        let mut g = cb.lock().unwrap();
                        if g.is_none() {
                            *g = Some(started.elapsed());
                        }
                        true
                    }),
                )
                .expect("tts generates");
            let total = started.elapsed();
            let first_audio = *cb_first.lock().unwrap();
            let secs = audio.samples().len() as f32 / audio.sample_rate() as f32;
            println!(
                "{label}: {secs:.1}s of audio, first audio {first_audio:?}, total {total:?}"
            );
            assert!(secs > 0.5);
        };

        let t = Instant::now();
        let kokoro = OfflineTts::create(&kokoro_config(&root.join("kokoro"), &root.join("kokoro/espeak-ng-data"), "model.int8.onnx", threads))
            .expect("kokoro loads");
        println!("kokoro int8 load: {:?}", t.elapsed());
        let en = "Sure, I can look that up for you and tell you what I find in a moment.";
        speak("kokoro en (cold)", &kokoro, en);
        speak("kokoro en (warm)", &kokoro, en);
        drop(kokoro);
        if root.join("kokoro32").exists() {
            let t = Instant::now();
            let k32 = OfflineTts::create(&kokoro_config(&root.join("kokoro32"), &root.join("kokoro32/espeak-ng-data"), "model.onnx", threads))
                .expect("kokoro fp32 loads");
            println!("kokoro fp32 load: {:?}", t.elapsed());
            speak("kokoro fp32 en (cold)", &k32, en);
            speak("kokoro fp32 en (warm)", &k32, en);
        }

        let t = Instant::now();
        let piper = OfflineTts::create(&piper_config(
            &root.join("piper-thorsten"),
            &root.join("piper-thorsten/espeak-ng-data"),
            "de_DE-thorsten-medium.onnx",
            threads,
        ))
        .expect("piper loads");
        println!("piper load: {:?}", t.elapsed());
        let de = "Klar, ich schaue das kurz nach und sage dir gleich, was ich gefunden habe.";
        speak("piper de (cold)", &piper, de);
        speak("piper de (warm)", &piper, de);
    }
}
