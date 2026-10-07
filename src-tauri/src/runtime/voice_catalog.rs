//! Voice catalog, hardware defaults and model install (VOC-2, VOC-3, VOC-4).
//!
//! Every entry points at a sherpa-onnx release asset whose name, size and
//! sha256 were read from the release (see "Build notes" in the voice plan).
//! Copy shown to the user follows VXP-6: no model or engine names, none of the
//! words in [`BANNED_WORDS`].

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::download::{download_with_resume, sha256_file, DownloadError, DownloadProgress};
use super::hardware::HardwareProfile;

const RELEASES: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download";

/// Words user-facing voice copy must not contain (VXP-6), checked in tests.
pub const BANNED_WORDS: &[&str] = &[
    "vad", "asr", "stt", "tts", "onnx", "endpointing", "barge-in", "transcription model",
    "embedding", "vector", "reranker",
];

/// A downloadable archive, checked by size and sha256 before it is unpacked.
#[derive(Debug, Clone, Serialize)]
pub struct ModelArchive {
    pub url: String,
    pub file_name: String,
    pub sha256: String,
    pub bytes: u64,
}

fn archive(tag: &str, file_name: &str, sha256: &str, bytes: u64) -> ModelArchive {
    ModelArchive {
        url: format!("{RELEASES}/{tag}/{file_name}"),
        file_name: file_name.into(),
        sha256: sha256.into(),
        bytes,
    }
}

/// Which recognizer family a hearing model belongs to; picks the config builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HearingFamily {
    Parakeet,
    Moonshine,
}

/// Something Poiesis understands speech with. Always installed together with
/// the small speech-detection file ([`vad_archive`]).
#[derive(Debug, Clone, Serialize)]
pub struct HearingEntry {
    /// Folder name under `<app data>/voice/`; also the value of `voice.hearing_model`.
    pub id: String,
    pub family: HearingFamily,
    pub name: String,
    pub note: String,
    pub size_label: String,
    pub languages: Vec<String>,
    pub archive: ModelArchive,
}

/// Which synthesis family a voice model belongs to; picks the config builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceFamily {
    Kokoro,
    Piper,
}

/// One selectable voice inside a voice model.
#[derive(Debug, Clone, Serialize)]
pub struct VoiceChoice {
    /// Value of `voice.voice_id`. Unique across the catalog.
    pub id: String,
    pub name: String,
    /// Speaker number inside the model.
    pub sid: i32,
    pub language: String,
    /// Language and region, such as `en_GB`; the screen shows the region only
    /// where a language has voices from more than one.
    pub locale: String,
}

/// A downloadable voice model holding one or more voices.
#[derive(Debug, Clone, Serialize)]
pub struct VoiceModelEntry {
    /// Folder name under `<app data>/voice/`.
    pub id: String,
    pub family: VoiceFamily,
    pub note: String,
    pub size_label: String,
    /// Short license of the voice data, shown next to it ("CC0", "CC BY 4.0").
    pub license: String,
    /// The `.onnx` file name inside the unpacked folder.
    pub model_file: String,
    pub archive: ModelArchive,
    pub voices: Vec<VoiceChoice>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VoiceCatalog {
    pub hearing: Vec<HearingEntry>,
    pub voices: Vec<VoiceModelEntry>,
}

/// The speech-detection file every hearing install needs. It is a bare
/// `.onnx`, not an archive, so it is stored as `silero_vad.onnx` in the voice
/// root rather than unpacked.
pub fn vad_archive() -> ModelArchive {
    archive(
        "asr-models",
        "silero_vad.onnx",
        "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6",
        643_854,
    )
}

fn choice(id: &str, name: &str, sid: i32, locale: &str) -> VoiceChoice {
    let language = locale.split('_').next().unwrap_or(locale);
    VoiceChoice { id: id.into(), name: name.into(), sid, language: language.into(), locale: locale.into() }
}

/// English name of a language code, for plain sentences. Covers every language
/// that has a voice in the catalog.
pub fn language_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "en" => "English",
        "de" => "German",
        "fr" => "French",
        "es" => "Spanish",
        "it" => "Italian",
        "nl" => "Dutch",
        "pt" => "Portuguese",
        "pl" => "Polish",
        "ru" => "Russian",
        "sv" => "Swedish",
        "da" => "Danish",
        "no" => "Norwegian",
        "fi" => "Finnish",
        "cs" => "Czech",
        "el" => "Greek",
        "hu" => "Hungarian",
        "ro" => "Romanian",
        "vi" => "Vietnamese",
        _ => return None,
    })
}

/// One Piper voice model from the sherpa-onnx `tts-models` release: the
/// archive name, size and sha256 were read from the release. Only voices whose
/// data license allows use and sharing without a non-commercial or share-alike
/// condition are listed. Each model's card is in `THIRD_PARTY_NOTICES.md`.
struct PiperRow {
    locale: &'static str,
    name: &'static str,
    quality: &'static str,
    license: &'static str,
    /// (name shown, speaker number)
    voices: &'static [(&'static str, i32)],
    sha256: &'static str,
    bytes: u64,
}

const PIPER: &[PiperRow] = &[
    PiperRow { locale: "en_GB", name: "alba", quality: "medium", license: "CC BY 4.0", voices: &[("Alba", 0)], sha256: "f7581d123ae977f64f3032bb247d4deeac8440e881d918d36fdd36d8f1030fb7", bytes: 21104326 },
    PiperRow { locale: "en_US", name: "ljspeech", quality: "medium", license: "Public domain", voices: &[("LJ", 0)], sha256: "24dc3bd77dd48c291e52c297878d3437c9492f245d823d7f6a06c4bbb67f4b6b", bytes: 21090429 },
    PiperRow { locale: "en_US", name: "joe", quality: "medium", license: "CC0", voices: &[("Joe", 0)], sha256: "644527f29eca0ada5595d7b6e4daf6388f5ac0c4870b4d42996a6687fcefeb37", bytes: 21230019 },
    PiperRow { locale: "en_US", name: "kristin", quality: "medium", license: "Public domain", voices: &[("Kristin", 0)], sha256: "16289d7ee8e6b2311a0a0af6531a55f498f82499644a1bb6fddb991fe6fa950c", bytes: 20882061 },
    PiperRow { locale: "en_US", name: "norman", quality: "medium", license: "Public domain", voices: &[("Norman", 0)], sha256: "cb481a514bc213ccf3899391c0f27fdcc4e4b814ec30496f28089a027b5aa01b", bytes: 20987233 },
    PiperRow { locale: "en_GB", name: "cori", quality: "medium", license: "Public domain", voices: &[("Cori", 0)], sha256: "169ca8aff3adb271f009a4924c99928a811dbf2b52eaca2dbb460e8c34478c93", bytes: 20768736 },
    PiperRow { locale: "de_DE", name: "thorsten", quality: "medium", license: "CC0", voices: &[("Thorsten", 0)], sha256: "07e240b7b9c1fc9211d5a69512f8cbe11b3286c2ed79c15c076ac6ed427fdf13", bytes: 20949833 },
    PiperRow { locale: "de_DE", name: "kerstin", quality: "low", license: "CC0", voices: &[("Kerstin", 0)], sha256: "bcd8039667940cf2efc939b844f4b33d0823096572fcc1a8caaa2faa77f3379c", bytes: 21174728 },
    PiperRow { locale: "fr_FR", name: "siwis", quality: "medium", license: "CC BY 4.0", voices: &[("Siwis", 0)], sha256: "3909cff9b3cfd4820c66aa13bf554315c82e34899c161f0b446ece372bc4b5ec", bytes: 20914888 },
    PiperRow { locale: "fr_FR", name: "gilles", quality: "low", license: "CC0", voices: &[("Gilles", 0)], sha256: "92d2bfd9b6b32b9787557c466b15620e453aecc10ec521762660e034941b3c43", bytes: 21248965 },
    PiperRow { locale: "es_ES", name: "davefx", quality: "medium", license: "CC0", voices: &[("Davefx", 0)], sha256: "8bb8ac1cefb727caec9bd9c6c3185c673c8b42c53bd29bb25d5a7715dac37125", bytes: 21171632 },
    PiperRow { locale: "es_ES", name: "carlfm", quality: "x_low", license: "Public domain", voices: &[("Carlfm", 0)], sha256: "fe5b74e55254e2a568a4e4d73fcfcc02580c7957fc337777adf46e5fdfe10218", bytes: 13356095 },
    PiperRow { locale: "es_MX", name: "ald", quality: "medium", license: "Unlicense", voices: &[("Ald", 0)], sha256: "447e82d080719409db08e54a4b6eec2e4b6ba850b98dcaf09d3fc67bf30ff692", bytes: 21283187 },
    PiperRow { locale: "es_MX", name: "claude", quality: "high", license: "Apache 2.0", voices: &[("Claude", 0)], sha256: "0f9fc9c07d2e17bdc0f5f33a657addae92085da85704cb86a861e59d32f3bbfa", bytes: 21216685 },
    PiperRow { locale: "it_IT", name: "paola", quality: "medium", license: "CC0", voices: &[("Paola", 0)], sha256: "2b975ed305391c056944a4dde67ee754dd824099503a860295bb4c1d724662d8", bytes: 21143212 },
    PiperRow { locale: "nl_NL", name: "pim", quality: "medium", license: "CC0", voices: &[("Pim", 0)], sha256: "10433635e34020dced7067b84880c2c7b91c1b9b861e9be8b1a832ead891880e", bytes: 21127183 },
    PiperRow { locale: "nl_NL", name: "ronnie", quality: "medium", license: "CC0", voices: &[("Ronnie", 0)], sha256: "eb16022c9c8ee48b75dc833e8a8b04e08730929ea68cde9b67e2dc712f988978", bytes: 21158108 },
    PiperRow { locale: "nl_BE", name: "nathalie", quality: "medium", license: "CC0", voices: &[("Nathalie", 0)], sha256: "f2cdeb555eacbdd053e5d63c4d0120464161a962f9fe3bf9aac47ecc57ac90ac", bytes: 21139955 },
    PiperRow { locale: "pt_BR", name: "faber", quality: "medium", license: "CC0", voices: &[("Faber", 0)], sha256: "dbc8b1d7d729fd417ea78a350ed35696c928770ac93513d3f507bd4e88eee3fd", bytes: 21336772 },
    PiperRow { locale: "pt_BR", name: "cadu", quality: "medium", license: "CC0", voices: &[("Cadu", 0)], sha256: "78f1caf0a74cc6cb8dedaff87affd232ee653d5b0394d4cf4d2e97ecbfa5ff3d", bytes: 21135464 },
    PiperRow { locale: "pt_BR", name: "jeff", quality: "medium", license: "CC0", voices: &[("Jeff", 0)], sha256: "21a8883f9662c784dd5653fd9d5cb9aaae2551c70e16854e81ff4a9c96470e6a", bytes: 21211448 },
    PiperRow { locale: "pl_PL", name: "gosia", quality: "medium", license: "CC0", voices: &[("Gosia", 0)], sha256: "72acac4c4b031725c41a61b3af0314a3d30e1ec2cd83ee410ea5f9e6d2d9d4fb", bytes: 21109262 },
    PiperRow { locale: "pl_PL", name: "darkman", quality: "medium", license: "CC0", voices: &[("Darkman", 0)], sha256: "0ec47b7d591e48913da887ea7e81287f4ef621d9c5aa1848700d2d736ed3f99c", bytes: 21078264 },
    PiperRow { locale: "ru_RU", name: "denis", quality: "medium", license: "CC0", voices: &[("Denis", 0)], sha256: "d710e29eb7854c42461d16d65d3cef753cc559639ca75806edc75ba71f23af66", bytes: 21058905 },
    PiperRow { locale: "ru_RU", name: "dmitri", quality: "medium", license: "CC0", voices: &[("Dmitri", 0)], sha256: "7636793307f634ce54c6e65528a91a61683114f1a6635a08caf64ba6c54e6a63", bytes: 21129441 },
    PiperRow { locale: "sv_SE", name: "nst", quality: "medium", license: "CC0", voices: &[("Nst", 0)], sha256: "ac568458aac847d44b7945ba06f9b352122ae718b50b43af924a7ded8be1a933", bytes: 20972387 },
    PiperRow { locale: "da_DK", name: "talesyntese", quality: "medium", license: "CC0", voices: &[("Tale", 0)], sha256: "83c5b433e97c86109d659d7adcfc865bdf5bc570baa1e1ce8357f4fbf7346a7e", bytes: 21025554 },
    PiperRow { locale: "no_NO", name: "talesyntese", quality: "medium", license: "CC0", voices: &[("Tale", 0)], sha256: "de0fc89178fabf0636554af8ae131b4dfe034866458bb7daa7026ee0d0d8d2f7", bytes: 21072816 },
    PiperRow { locale: "fi_FI", name: "harri", quality: "medium", license: "CC0", voices: &[("Harri", 0)], sha256: "b69b4c4465787dd4b9778d16b812ed7577a4051551cd87b88e0762309499c3c5", bytes: 20984753 },
    PiperRow { locale: "cs_CZ", name: "jirka", quality: "medium", license: "CC0", voices: &[("Jirka", 0)], sha256: "45377b35ce823eaac5d76a5530b48fb5ad386a4e07db6ff36aa7c417d6bd0a6d", bytes: 21002417 },
    PiperRow { locale: "el_GR", name: "rapunzelina", quality: "low", license: "CC0", voices: &[("Rapunzelina", 0)], sha256: "1fbab58d6380dda2a392ef531de89e8d1202d2dca804db7ebcff5be2b30c823a", bytes: 21090546 },
    PiperRow { locale: "hu_HU", name: "anna", quality: "medium", license: "CC0", voices: &[("Anna", 0)], sha256: "23f9e9fd59ae28fe04796c84079ed1bcf474e1bd35b97c55447fcc3c4e5e42e1", bytes: 21113090 },
    PiperRow { locale: "ro_RO", name: "mihai", quality: "medium", license: "CC0", voices: &[("Mihai", 0)], sha256: "1f41b684fb4640d1b79f750f1581647f8737b24ff5daf8d7c5fd5ad6d1761499", bytes: 21081899 },
    PiperRow { locale: "vi_VN", name: "vais1000", quality: "medium", license: "CC BY 4.0", voices: &[("Vais", 0)], sha256: "60e8b82a27cfb9bfa527f5789f0879d02ec73a7397659973b937c823c326d5dd", bytes: 21574925 },
];

fn piper_entry(row: &PiperRow) -> VoiceModelEntry {
    let lang = row.locale.split('_').next().unwrap_or(row.locale);
    let id = format!("piper-{lang}-{}", row.name.replace('_', "-"));
    let file_name = format!("vits-piper-{}-{}-{}-int8.tar.bz2", row.locale, row.name, row.quality);
    let size_label = format!("About {} MB", (row.bytes as f64 / 1_000_000.0).round());
    let language = language_name(lang).unwrap_or("");
    let (noun, verb) = if row.voices.len() > 1 { ("voices", "run") } else { ("voice", "runs") };
    let note = format!("A light {language} {noun} that {verb} on any computer. {size_label}.");
    VoiceModelEntry {
        voices: row
            .voices
            .iter()
            .enumerate()
            .map(|(i, (name, sid))| {
                // The first voice carries the model's id, so the id never changes.
                let vid = if i == 0 { id.clone() } else { format!("{id}-{}", name.to_lowercase()) };
                choice(&vid, name, *sid, row.locale)
            })
            .collect(),
        id,
        family: VoiceFamily::Piper,
        note,
        size_label,
        license: row.license.into(),
        model_file: format!("{}-{}-{}.onnx", row.locale, row.name, row.quality),
        archive: archive("tts-models", &file_name, row.sha256, row.bytes),
    }
}

pub fn voice_catalog() -> VoiceCatalog {
    VoiceCatalog {
        hearing: vec![HearingEntry {
            id: "parakeet-v3".into(),
            family: HearingFamily::Parakeet,
            name: "Standard hearing".into(),
            note: "Understands German, English and 23 more European languages, with punctuation. About 490 MB."
                .into(),
            size_label: "About 490 MB".into(),
            languages: ["bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv", "lt", "mt", "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            archive: archive(
                "asr-models",
                "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2",
                "5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf",
                487_170_055,
            ),
        },
        // English only (its German sibling is non-commercial, so not offered).
        // Small enough for computers with little memory. License: MIT.
        HearingEntry {
            id: "moonshine-en".into(),
            family: HearingFamily::Moonshine,
            name: "Light hearing".into(),
            note: "Understands English only and runs on small computers. About 110 MB.".into(),
            size_label: "About 110 MB".into(),
            languages: vec!["en".into()],
            archive: archive(
                "asr-models",
                "sherpa-onnx-moonshine-base-en-quantized-2026-02-27.tar.bz2",
                "43232c1d13013d37317163baec3135bd771a186a4356f28c889bab453bb0e891",
                111_266_225,
            ),
        }],
        voices: std::iter::once(VoiceModelEntry {
                id: "kokoro-en".into(),
                family: VoiceFamily::Kokoro,
                note: "Natural English voices. Needs a reasonably fast computer. About 350 MB.".into(),
                size_label: "About 350 MB".into(),
                license: "Apache 2.0".into(),
                model_file: "model.onnx".into(),
                archive: archive(
                    "tts-models",
                    "kokoro-multi-lang-v1_0.tar.bz2",
                    "c5f7e2d2caf082bc1d20fb70334a61d99d20b484500aad32e7cf84c128ea3298",
                    349_906_910,
                ),
                // Speaker numbers read from the model's own metadata.
                voices: vec![
                    choice("af_heart", "Heart", 3, "en_US"),
                    choice("af_bella", "Bella", 2, "en_US"),
                    choice("af_nova", "Nova", 7, "en_US"),
                    choice("am_michael", "Michael", 16, "en_US"),
                    choice("am_puck", "Puck", 18, "en_US"),
                    choice("bf_emma", "Emma", 21, "en_GB"),
                    choice("bm_george", "George", 26, "en_GB"),
                ],
            })
            .chain(PIPER.iter().map(piper_entry))
            .collect(),
    }
}

/// The hearing id and voice model id to offer first on this machine (VOC-3).
/// `language` is the UI language code (`en`, `de`, ...). A language with a
/// voice in the catalog gets its first voice; any other gets English.
///
/// Kokoro is English only and about three times slower than real time on weak
/// CPUs when int8; the fp32 build in the catalog needs a machine with at least
/// 8 GB RAM. Everything else falls back to the light Piper voices. The light
/// hearing is English only, so it is the default only for English on a small
/// machine; any other language keeps the standard hearing.
pub fn default_choice(hw: &HardwareProfile, language: &str) -> (&'static str, String) {
    let small = hw.ram_mb < 8 * 1024;
    let code = language.split(['-', '_']).next().unwrap_or("");
    let own = voice_catalog()
        .voices
        .into_iter()
        .find(|m| code != "en" && m.voices.first().is_some_and(|v| v.language == code))
        .map(|m| m.id);
    let voice = match own {
        Some(id) => id,
        None if !small => "kokoro-en".to_string(),
        None => "piper-en-alba".to_string(),
    };
    let hearing = if small && code == "en" { "moonshine-en" } else { "parakeet-v3" };
    (hearing, voice)
}

/// Thread count for speech engines: half the physical cores, at most 4, at
/// least 1 (VOC-3).
pub fn default_threads(hw: &HardwareProfile) -> i32 {
    (hw.cpu.physical_cores / 2).clamp(1, 4) as i32
}

/// Where voice files live: `<app data>/voice/`.
#[derive(Debug, Clone)]
pub struct VoicePaths {
    root: PathBuf,
}

impl VoicePaths {
    pub fn new(app_data: &Path) -> Self {
        Self { root: app_data.join("voice") }
    }
    pub fn model_dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }
    pub fn vad_file(&self) -> PathBuf {
        self.root.join("silero_vad.onnx")
    }
    fn downloads_dir(&self) -> PathBuf {
        self.root.join("downloads")
    }
    pub fn is_installed(&self, id: &str) -> bool {
        self.model_dir(id).join(".installed").exists()
    }

    /// The one speech-data folder every voice uses. The speech engine reads its
    /// data folder once per program run and keeps that path, so it must not be
    /// inside a voice that can be removed. It is copied from the first voice
    /// that has one (about 18 MB) and never deleted.
    pub fn espeak_dir(&self) -> PathBuf {
        self.root.join("espeak-ng-data")
    }

    /// Data folder for a voice model: the shared one, made now if this model
    /// brings it and no other has. Falls back to the model's own copy.
    pub fn espeak_for(&self, model_id: &str) -> PathBuf {
        let shared = self.espeak_dir();
        let own = self.model_dir(model_id).join("espeak-ng-data");
        if !shared.exists() && own.exists() {
            let staging = self.root.join("espeak-ng-data.partial");
            let _ = fs::remove_dir_all(&staging);
            if copy_dir(&own, &staging).is_ok() && fs::rename(&staging, &shared).is_err() {
                let _ = fs::remove_dir_all(&staging);
            }
        }
        if shared.exists() {
            shared
        } else {
            own
        }
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Unpack a `.tar.bz2` into `dest`. The `tar` crate refuses entries that would
/// land outside `dest` (`..` or absolute paths).
pub fn unpack_tar_bz2(archive: &Path, dest: &Path) -> Result<(), DownloadError> {
    let file = fs::File::open(archive)?;
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(file));
    tar.unpack(dest).map_err(|e| DownloadError::Archive(e.to_string()))
}

async fn fetch_verified<F>(
    client: &reqwest::Client,
    paths: &VoicePaths,
    item: &ModelArchive,
    label: &str,
    on_progress: F,
) -> Result<PathBuf, DownloadError>
where
    F: FnMut(DownloadProgress),
{
    let dest = paths.downloads_dir().join(&item.file_name);
    download_with_resume(client, &item.url, &dest, label, on_progress).await?;
    let check = dest.clone();
    let actual = tauri::async_runtime::spawn_blocking(move || sha256_file(&check))
        .await
        .map_err(|e| DownloadError::Archive(e.to_string()))??;
    if !actual.eq_ignore_ascii_case(&item.sha256) {
        // A bad file must not be resumed or reused.
        let _ = fs::remove_file(&dest);
        return Err(DownloadError::Checksum { expected: item.sha256.clone(), actual });
    }
    Ok(dest)
}

/// Download, verify and unpack a model archive into `<voice>/<id>/`. The
/// archive holds one top folder; its contents become the model folder. A
/// `.installed` marker is written last, so a half-finished install never looks
/// complete.
pub async fn install_archive<F>(
    client: &reqwest::Client,
    paths: &VoicePaths,
    id: &str,
    item: &ModelArchive,
    label: &str,
    on_progress: F,
) -> Result<(), DownloadError>
where
    F: FnMut(DownloadProgress),
{
    let file = fetch_verified(client, paths, item, label, on_progress).await?;
    let final_dir = paths.model_dir(id);
    let staging = paths.root.join(format!("{id}.partial"));
    let (src, stage, dest) = (file.clone(), staging.clone(), final_dir.clone());
    tauri::async_runtime::spawn_blocking(move || -> Result<(), DownloadError> {
        let _ = fs::remove_dir_all(&stage);
        let _ = fs::remove_dir_all(&dest);
        fs::create_dir_all(&stage)?;
        unpack_tar_bz2(&src, &stage)?;
        // One top folder: move it into place.
        let mut tops = fs::read_dir(&stage)?.filter_map(|e| e.ok()).collect::<Vec<_>>();
        let inner = match tops.pop() {
            Some(e) if tops.is_empty() && e.path().is_dir() => e.path(),
            _ => return Err(DownloadError::Archive("unexpected archive layout".into())),
        };
        fs::rename(&inner, &dest)?;
        let _ = fs::remove_dir_all(&stage);
        fs::write(dest.join(".installed"), b"ok")?;
        Ok(())
    })
    .await
    .map_err(|e| DownloadError::Archive(e.to_string()))??;
    let _ = fs::remove_file(file);
    Ok(())
}

/// Download and verify the speech-detection file.
pub async fn install_vad<F>(
    client: &reqwest::Client,
    paths: &VoicePaths,
    on_progress: F,
) -> Result<(), DownloadError>
where
    F: FnMut(DownloadProgress),
{
    let item = vad_archive();
    let file = fetch_verified(client, paths, &item, "Getting ready to listen", on_progress).await?;
    fs::rename(&file, paths.vad_file())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::hardware::{CpuInfo, HardwareProfile};

    fn hw(ram_mb: u64, cores: usize) -> HardwareProfile {
        HardwareProfile {
            cpu: CpuInfo { brand: "test".into(), physical_cores: cores, avx2: true, avx512: false },
            ram_mb,
            gpus: vec![],
        }
    }

    fn all_notes(c: &VoiceCatalog) -> Vec<String> {
        let mut notes: Vec<String> = c.hearing.iter().flat_map(|h| [h.name.clone(), h.note.clone()]).collect();
        for v in &c.voices {
            notes.push(v.note.clone());
            notes.extend(v.voices.iter().map(|x| x.name.clone()));
        }
        notes
    }

    /// VOC-T3: every entry is complete and its copy passes the plain-words rule.
    #[test]
    fn every_entry_is_complete_and_its_copy_is_plain() {
        let c = voice_catalog();
        let mut archives: Vec<&ModelArchive> = c.hearing.iter().map(|h| &h.archive).collect();
        archives.extend(c.voices.iter().map(|v| &v.archive));
        for a in archives {
            assert!(a.url.starts_with("https://github.com/k2-fsa/sherpa-onnx/releases/download/"), "{}", a.url);
            assert!(a.url.ends_with(&a.file_name));
            assert_eq!(a.sha256.len(), 64, "{}", a.file_name);
            assert!(a.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(a.bytes > 0);
        }
        for h in &c.hearing {
            assert!(h.size_label.contains("MB") && !h.languages.is_empty());
        }
        for v in &c.voices {
            assert!(v.size_label.contains("MB") && v.model_file.ends_with(".onnx") && !v.voices.is_empty());
        }
        for note in all_notes(&c) {
            let lower = note.to_lowercase();
            for word in BANNED_WORDS {
                let hit = lower
                    .split(|ch: char| !ch.is_alphanumeric() && ch != '-')
                    .any(|w| w == *word)
                    || (word.contains(' ') && lower.contains(word));
                assert!(!hit, "'{word}' in user-facing copy: {note}");
            }
            for name in ["parakeet", "moonshine", "kokoro", "piper", "sherpa", "silero"] {
                assert!(!lower.contains(name), "'{name}' in user-facing copy: {note}");
            }
        }
    }

    /// Every voice we offer is credited with its license in THIRD_PARTY_NOTICES.md,
    /// and none carries a license that forbids commercial use or demands share-alike.
    #[test]
    fn every_piper_voice_is_credited_with_a_free_license() {
        let notices = include_str!("../../../THIRD_PARTY_NOTICES.md");
        let allowed = ["CC0", "Public domain", "CC BY 4.0", "Apache 2.0", "Unlicense"];
        for row in PIPER {
            let key = format!("{}-{}-{}", row.locale, row.name, row.quality);
            assert!(notices.contains(&key), "{key} is not in THIRD_PARTY_NOTICES.md");
            assert!(allowed.contains(&row.license), "{key}: license {}", row.license);
            assert!(language_name(row.locale.split('_').next().unwrap()).is_some(), "{key}: language has no name");
        }
    }

    #[test]
    fn many_languages_are_offered_and_ids_stay_what_they_were() {
        let c = voice_catalog();
        let mut languages: Vec<&str> = c.voices.iter().flat_map(|m| m.voices.iter().map(|v| v.language.as_str())).collect();
        languages.sort();
        languages.dedup();
        assert!(languages.len() >= 15, "{languages:?}");
        // Saved settings point at these.
        for id in ["af_heart", "piper-en-alba", "piper-de-thorsten"] {
            assert!(c.voices.iter().any(|m| m.voices.iter().any(|v| v.id == id)), "{id}");
        }
        assert!(c.voices.iter().all(|m| !m.license.is_empty()));
    }

    #[test]
    fn voice_ids_are_unique() {
        let c = voice_catalog();
        let mut ids: Vec<&str> = c.voices.iter().flat_map(|m| m.voices.iter().map(|v| v.id.as_str())).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn defaults_follow_language_and_memory() {
        assert_eq!(default_choice(&hw(16 * 1024, 8), "en-US"), ("parakeet-v3", "kokoro-en".to_string()));
        assert_eq!(default_choice(&hw(4 * 1024, 4), "en"), ("moonshine-en", "piper-en-alba".to_string()));
        assert_eq!(default_choice(&hw(16 * 1024, 8), "de-DE").1, "piper-de-thorsten");
        // The light hearing knows English only, so German keeps the standard one.
        assert_eq!(default_choice(&hw(4 * 1024, 4), "de"), ("parakeet-v3", "piper-de-thorsten".to_string()));
        // An unknown UI language is not assumed to be English.
        assert_eq!(default_choice(&hw(4 * 1024, 4), "fr").0, "parakeet-v3");
        // Another language gets its own voice; one with no voice falls back to English.
        assert_eq!(default_choice(&hw(16 * 1024, 8), "fr-FR").1, "piper-fr-siwis");
        assert_eq!(default_choice(&hw(16 * 1024, 8), "pt_BR").1, "piper-pt-faber");
        assert_eq!(default_choice(&hw(16 * 1024, 8), "ja").1, "kokoro-en");
    }

    #[test]
    fn threads_are_half_the_cores_between_one_and_four() {
        assert_eq!(default_threads(&hw(0, 1)), 1);
        assert_eq!(default_threads(&hw(0, 4)), 2);
        assert_eq!(default_threads(&hw(0, 8)), 4);
        assert_eq!(default_threads(&hw(0, 32)), 4);
    }

    #[test]
    fn unpack_lands_inside_the_destination() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let tar_path = tmp.path().join("t.tar.bz2");
        {
            let enc = bzip2::write::BzEncoder::new(fs::File::create(&tar_path).unwrap(), bzip2::Compression::fast());
            let mut b = tar::Builder::new(enc);
            let data = b"hello";
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, "top/a.txt", &data[..]).unwrap();
            b.into_inner().unwrap().finish().unwrap().flush().unwrap();
        }
        let out = tmp.path().join("out");
        fs::create_dir_all(&out).unwrap();
        unpack_tar_bz2(&tar_path, &out).unwrap();
        assert_eq!(fs::read(out.join("top/a.txt")).unwrap(), b"hello");
    }

    #[test]
    fn voices_share_one_speech_data_folder_that_outlives_them() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = VoicePaths::new(tmp.path());
        // Nothing installed: nothing to share, the model's own path is returned.
        assert_eq!(paths.espeak_for("a"), paths.model_dir("a").join("espeak-ng-data"));
        for id in ["a", "b"] {
            fs::create_dir_all(paths.model_dir(id).join("espeak-ng-data/sub")).unwrap();
            fs::write(paths.model_dir(id).join("espeak-ng-data/sub/x_dict"), id).unwrap();
        }
        assert_eq!(paths.espeak_for("a"), paths.espeak_dir());
        // The first voice's copy is kept for everyone, and survives its removal.
        fs::remove_dir_all(paths.model_dir("a")).unwrap();
        assert_eq!(paths.espeak_for("b"), paths.espeak_dir());
        assert_eq!(fs::read_to_string(paths.espeak_dir().join("sub/x_dict")).unwrap(), "a");
    }

    #[test]
    fn a_folder_counts_as_installed_only_with_its_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = VoicePaths::new(tmp.path());
        fs::create_dir_all(paths.model_dir("x")).unwrap();
        assert!(!paths.is_installed("x"));
        fs::write(paths.model_dir("x").join(".installed"), b"ok").unwrap();
        assert!(paths.is_installed("x"));
    }
}
