//! `TIER-2`: the context window a local model loads with.
//!
//! Every local model used to load with 4096 tokens, a fallback from the baseline
//! commit that no plan gave a reason for. A model trained for 32K or 128K still
//! got 4K. This picks what the model was trained for, limited by the memory the
//! PC has. Pure: the commands gather the facts and call [`choose_window`].

use super::gguf::GgufMeta;
use super::hardware::HardwareProfile;
use super::process::EngineError;

/// What a model loads with when its file cannot be read: the value used before
/// this plan, so a file we cannot read behaves exactly as it did.
pub const DEFAULT_WINDOW: u32 = 4096;
/// Full needs no more than this (`TIER-3`), and a bigger window costs memory and
/// speed.
pub const WINDOW_CAP: u32 = 32768;
pub const WINDOW_FLOOR: u32 = 2048;
/// Compute buffers and the like, which the weights and the KV cache do not cover.
const RESERVE_BYTES: u64 = 1 << 30;
const STEP: u32 = 1024;

/// A model another local server holds in memory while this one loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resident {
    pub bytes: u64,
    /// Whether it sits in video memory. The embedding and rerank engines are
    /// CPU-only by design, so they take RAM and never VRAM.
    pub on_gpu: bool,
}

/// KV cache bytes per token: for every layer, the KV heads times the key and
/// value lengths, at 2 bytes each (f16, the llama-server default at `b11418`).
///
/// For a sliding-window model this overestimates the cache. That is the safe
/// direction: the window comes out smaller, never too big.
pub fn kv_bytes_per_token(meta: &GgufMeta) -> u64 {
    let per_head = u64::from(meta.key_length) + u64::from(meta.value_length);
    meta.head_count_kv.iter().map(|h| u64::from(*h) * per_head * 2).sum()
}

/// Memory the model's KV cache may use. `HardwareProfile` reports total, not
/// free, memory, so this is built by subtraction: the card (or half the RAM when
/// there is no card or its size is unknown), minus the weights, minus the other
/// local models that share that memory, minus a reserve.
pub fn kv_budget(hw: &HardwareProfile, model_bytes: u64, others: &[Resident]) -> u64 {
    const MB: u64 = 1 << 20;
    let vram = hw.vram_mb() * MB;
    let on_gpu = vram > 0;
    let pool = if on_gpu { vram } else { hw.ram_mb * MB / 2 };
    let shared: u64 = others.iter().filter(|o| o.on_gpu == on_gpu || !on_gpu).map(|o| o.bytes).sum();
    pool.saturating_sub(model_bytes).saturating_sub(shared).saturating_sub(RESERVE_BYTES)
}

/// The window to load `meta` with.
///
/// 1. An override wins, but never above what the model was trained for.
/// 2. Start from the trained size. Unknown is [`DEFAULT_WINDOW`].
/// 3. Keep what the memory left for the KV cache can hold.
/// 4. At most [`WINDOW_CAP`], rounded down to a multiple of 1024, at least
///    [`WINDOW_FLOOR`].
pub fn choose_window(
    meta: Option<&GgufMeta>,
    hw: &HardwareProfile,
    model_bytes: u64,
    others: &[Resident],
    override_: Option<u32>,
) -> u32 {
    let Some(meta) = meta else {
        return override_.unwrap_or(DEFAULT_WINDOW);
    };
    if let Some(o) = override_ {
        return o.min(meta.trained_ctx);
    }
    let mut window = meta.trained_ctx.min(WINDOW_CAP);
    let per_token = kv_bytes_per_token(meta);
    if let Some(fits) = kv_budget(hw, model_bytes, others).checked_div(per_token) {
        window = window.min(u32::try_from(fits).unwrap_or(u32::MAX));
    }
    (window / STEP * STEP).max(WINDOW_FLOOR).min(meta.trained_ctx)
}

/// The next window to try after a load failed: half, kept to a multiple of 1024.
/// `None` once the floor has been tried.
pub fn smaller(window: u32) -> Option<u32> {
    (window > WINDOW_FLOOR).then(|| (window / 2 / STEP * STEP).max(WINDOW_FLOOR))
}

/// Load with `window`, and if the engine dies while starting, try again with half
/// of it, down to the floor, before giving the error back. The estimate can be
/// wrong, for example when another program is using the GPU.
///
/// Only a start that died is retried. A timeout takes two minutes to report, and
/// a missing binary does not depend on the window. Returns what the loader
/// returned together with the window that loaded.
pub async fn load_shrinking<T, F, Fut>(window: u32, shrink: bool, mut load: F) -> Result<(T, u32), EngineError>
where
    F: FnMut(u32) -> Fut,
    Fut: std::future::Future<Output = Result<T, EngineError>>,
{
    let mut window = window;
    loop {
        match load(window).await {
            Ok(done) => return Ok((done, window)),
            Err(EngineError::ExitedEarly(why)) => match smaller(window).filter(|_| shrink) {
                Some(next) => window = next,
                None => return Err(EngineError::ExitedEarly(why)),
            },
            Err(other) => return Err(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::hardware::{CpuInfo, GpuInfo, GpuVendor};

    const GB: u64 = 1 << 30;

    fn hw(ram_gb: u64, vram_gb: Option<u64>) -> HardwareProfile {
        HardwareProfile {
            cpu: CpuInfo { brand: "x".into(), physical_cores: 8, avx2: true, avx512: false },
            ram_mb: ram_gb * 1024,
            gpus: vram_gb
                .map(|v| {
                    vec![GpuInfo { vendor: GpuVendor::Nvidia, name: "g".into(), vram_mb: Some(v * 1024), driver_version: None }]
                })
                .unwrap_or_default(),
        }
    }

    /// 32 layers, 8 KV heads, 128 wide: 128 KiB of cache per token.
    fn meta(trained: u32) -> GgufMeta {
        GgufMeta {
            arch: "llama".into(),
            trained_ctx: trained,
            n_layer: 32,
            n_embd: 4096,
            head_count: vec![32; 32],
            head_count_kv: vec![8; 32],
            key_length: 128,
            value_length: 128,
            template_tools: Some(true),
        }
    }

    #[test]
    fn the_cache_per_token_adds_up_every_layer() {
        assert_eq!(kv_bytes_per_token(&meta(1)), 32 * 8 * 256 * 2);
        let mut hybrid = meta(1);
        hybrid.head_count_kv = vec![8, 0, 8, 0];
        assert_eq!(kv_bytes_per_token(&hybrid), 2 * 8 * 256 * 2, "a layer with 0 has no cache");
    }

    #[test]
    fn an_override_wins_but_never_above_the_trained_size() {
        let m = meta(16384);
        assert_eq!(choose_window(Some(&m), &hw(16, Some(8)), 4 * GB, &[], Some(8192)), 8192);
        assert_eq!(choose_window(Some(&m), &hw(16, Some(8)), 4 * GB, &[], Some(131072)), 16384);
        assert_eq!(choose_window(None, &hw(16, None), 0, &[], Some(8192)), 8192, "no metadata to cap it");
    }

    #[test]
    fn trained_size_below_what_the_memory_allows() {
        // 24 GB card, 4 GB model: plenty left for 8K at 128 KiB a token.
        assert_eq!(choose_window(Some(&meta(8192)), &hw(32, Some(24)), 4 * GB, &[], None), 8192);
    }

    #[test]
    fn memory_below_the_trained_size() {
        // 8 GB card, 4 GB model, 1 GB reserve: 3 GB of cache, 24576 tokens.
        let w = choose_window(Some(&meta(131072)), &hw(32, Some(8)), 4 * GB, &[], None);
        assert_eq!(w, 24576);
        // 6 GB card: 1 GB of cache, 8192 tokens.
        assert_eq!(choose_window(Some(&meta(131072)), &hw(32, Some(6)), 4 * GB, &[], None), 8192);
    }

    #[test]
    fn unknown_metadata_gives_the_old_value() {
        assert_eq!(choose_window(None, &hw(64, Some(24)), 4 * GB, &[], None), 4096);
    }

    #[test]
    fn no_gpu_uses_half_the_ram() {
        // 16 GB RAM is an 8 GB pool: 8 - 4 - 1 = 3 GB of cache.
        assert_eq!(choose_window(Some(&meta(131072)), &hw(16, None), 4 * GB, &[], None), 24576);
        // A GPU whose size is unknown counts as no GPU.
        let mut unknown = hw(16, Some(8));
        unknown.gpus[0].vram_mb = None;
        assert_eq!(choose_window(Some(&meta(131072)), &unknown, 4 * GB, &[], None), 24576);
    }

    #[test]
    fn the_other_servers_models_are_taken_off() {
        let h = hw(32, Some(8));
        let alone = choose_window(Some(&meta(131072)), &h, 4 * GB, &[], None);
        // A decision model in video memory takes 2 GB of the card.
        let gpu = Resident { bytes: 2 * GB, on_gpu: true };
        assert_eq!(choose_window(Some(&meta(131072)), &h, 4 * GB, &[gpu], None), 8192);
        assert!(alone > 8192);
        // The CPU-only embedder does not share the card.
        let cpu = Resident { bytes: 2 * GB, on_gpu: false };
        assert_eq!(choose_window(Some(&meta(131072)), &h, 4 * GB, &[cpu], None), alone);
        // With no card everything shares the RAM.
        let no_gpu = hw(16, None);
        let before = choose_window(Some(&meta(131072)), &no_gpu, 4 * GB, &[], None);
        assert!(choose_window(Some(&meta(131072)), &no_gpu, 4 * GB, &[cpu], None) < before);
    }

    #[test]
    fn capped_rounded_and_floored() {
        // Plenty of memory and a 128K model: the cap.
        assert_eq!(choose_window(Some(&meta(131072)), &hw(128, Some(80)), 4 * GB, &[], None), WINDOW_CAP);
        // A model bigger than the card: the floor, not zero.
        assert_eq!(choose_window(Some(&meta(131072)), &hw(32, Some(4)), 8 * GB, &[], None), WINDOW_FLOOR);
        // A trained size that is not a multiple of 1024 rounds down.
        assert_eq!(choose_window(Some(&meta(10_000)), &hw(64, Some(24)), 4 * GB, &[], None), 9216);
        // No cache at all (no KV heads) is limited by the trained size only.
        let mut none = meta(8192);
        none.head_count_kv = vec![0; 32];
        assert_eq!(choose_window(Some(&none), &hw(8, None), 20 * GB, &[], None), 8192);
        // The floor never goes above what the model was trained for.
        assert_eq!(choose_window(Some(&meta(1024)), &hw(64, Some(24)), GB, &[], None), 1024);
    }

    #[test]
    fn halving_stops_at_the_floor() {
        assert_eq!(smaller(32768), Some(16384));
        assert_eq!(smaller(5000), Some(2048));
        assert_eq!(smaller(4096), Some(2048));
        assert_eq!(smaller(2048), None);
    }

    fn died() -> EngineError {
        EngineError::ExitedEarly("out of memory".into())
    }

    #[tokio::test]
    async fn a_load_that_dies_is_retried_with_half_the_window() {
        let mut tried = Vec::new();
        let out = load_shrinking(16384, true, |w| {
            tried.push(w);
            async move { if w > 8192 { Err(died()) } else { Ok("up") } }
        })
        .await;
        assert_eq!(out.unwrap(), ("up", 8192), "the window that loaded comes back");
        assert_eq!(tried, vec![16384, 8192]);
    }

    #[tokio::test]
    async fn a_load_that_never_works_gives_the_error_after_the_floor() {
        let mut tried = Vec::new();
        let out: Result<((), u32), _> = load_shrinking(8192, true, |w| {
            tried.push(w);
            async { Err(died()) }
        })
        .await;
        assert!(matches!(out, Err(EngineError::ExitedEarly(_))));
        assert_eq!(tried, vec![8192, 4096, 2048]);
    }

    #[tokio::test]
    async fn nothing_is_retried_when_the_window_was_asked_for() {
        let mut tried = 0;
        let out: Result<((), u32), _> = load_shrinking(8192, false, |_| {
            tried += 1;
            async { Err(died()) }
        })
        .await;
        assert!(out.is_err());
        assert_eq!(tried, 1);
    }

    #[tokio::test]
    async fn a_timeout_is_not_retried() {
        let mut tried = 0;
        let out: Result<((), u32), _> = load_shrinking(8192, true, |_| {
            tried += 1;
            async { Err(EngineError::Timeout(std::time::Duration::from_secs(120), "slow".into())) }
        })
        .await;
        assert!(matches!(out, Err(EngineError::Timeout(..))));
        assert_eq!(tried, 1);
    }
}
