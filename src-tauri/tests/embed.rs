//! EMB — embedding engine integration test (`plans/PERCEPTION_PLAN.md` EMB).
//! Spawns a real embedding-mode `llama-server` and confirms 200 embedded
//! strings all come back unit-normalised (EMB-3). Needs a real llama-server
//! binary and a real embedding GGUF on disk, so it's `#[ignore]`d like `eval`:
//!
//! ```text
//! EMBED_SERVER_BIN=C:\path\to\llama-server.exe \
//! EMBED_MODEL_PATH=C:\path\to\bge-small-en-v1.5-f16.gguf \
//! cargo test --ignored embeds_200_strings -- --nocapture
//! ```

use std::path::PathBuf;

use poiesis_lib::runtime::embedserver::EmbedManager;

#[tokio::test]
#[ignore]
async fn embeds_200_strings_as_unit_vectors() {
    let server_binary = PathBuf::from(
        std::env::var("EMBED_SERVER_BIN").expect("set EMBED_SERVER_BIN to a llama-server(.exe) path"),
    );
    let model_path = PathBuf::from(
        std::env::var("EMBED_MODEL_PATH").expect("set EMBED_MODEL_PATH to a GGUF embedding model"),
    );

    let client = reqwest::Client::new();
    let mgr = EmbedManager::new();

    // 200 texts is seven batches, so this also covers reassembling batched
    // responses back into the caller's order.
    let texts: Vec<String> = (0..200)
        .map(|i| format!("sample sentence number {i} about nothing in particular"))
        .collect();
    let vectors = mgr
        .embed_texts(&client, server_binary, model_path, &texts)
        .await
        .expect("embed should succeed");

    assert_eq!(vectors.len(), 200);
    let dim = vectors[0].len();
    assert!(dim > 0, "embeddings should not be empty");
    for v in &vectors {
        assert_eq!(v.len(), dim, "every embedding should have the same dimension");
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-3, "embedding should be unit-normalised, got norm {norm}");
    }
    assert!(mgr.status().await.running, "the engine should still be up right after use");

    mgr.stop().await;
    assert!(!mgr.status().await.running, "stop() should take the engine down");
}
