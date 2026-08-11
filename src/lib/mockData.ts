// Placeholder data so the UI is fully explorable before backend phases land.
// Mirrors the conversation in Project_Poiesis_UI_Mockup.html. Replaced by real
// SQLite-backed data in Phase 2.

import type { Conversation, Model } from "./types";

export const mockModels: Model[] = [
  { id: "llama-3.1-8b", name: "Llama 3.1 8B", provenance: "local", meta: "fast", available: true },
  {
    id: "qwen-2.5-32b",
    name: "Qwen 2.5 32B",
    provenance: "local",
    meta: "stronger, slower",
    available: true,
  },
  { id: "claude", name: "Claude", provenance: "cloud", meta: "Anthropic", available: true },
  { id: "gpt-5", name: "GPT-5", provenance: "cloud", meta: "OpenAI", available: true },
];

export const mockConversations: Conversation[] = [
  {
    id: "c1",
    title: "Parsing CSV in Rust",
    updatedAt: Date.now(),
    messages: [
      {
        id: "m1",
        role: "user",
        text: "Find any files in my project that still reference the old API endpoint, and check if there's a newer version of the csv crate I should switch to.",
        createdAt: Date.now() - 60000,
      },
      {
        id: "m2",
        role: "assistant",
        model: { name: "Llama 3.1 8B", provenance: "local" },
        steps: [
          {
            id: "s1",
            verb: "searched",
            target: 'project files for "old-api.internal"',
            result: "— 3 matches",
            status: "done",
          },
          { id: "s2", verb: "read", target: "src/client.rs, src/sync.rs", status: "done" },
          {
            id: "s3",
            verb: "checked",
            target: "crates.io for csv",
            result: "— 1.3.1 is current, you have 1.2.0",
            status: "done",
          },
        ],
        text: "Two files still call the old endpoint: `src/client.rs` (line 41) and `src/sync.rs` (line 12). Both use a hardcoded URL rather than the config value, which is probably why they were missed in the last migration.\n\nThe `csv` crate has a minor update available, 1.3.1, with no breaking changes from 1.2.0 — safe to bump in `Cargo.toml` whenever you next touch dependencies.",
        createdAt: Date.now() - 50000,
      },
      {
        id: "m3",
        role: "user",
        text: "Good catch. Can you fix those two and also look up how my competitor structures their pricing page for inspiration?",
        createdAt: Date.now() - 30000,
      },
      {
        id: "m4",
        role: "assistant",
        model: { name: "Claude", provenance: "cloud" },
        steps: [
          {
            id: "s4",
            verb: "edited",
            target: "src/client.rs, src/sync.rs",
            result: "— replaced hardcoded URL with config value",
            status: "done",
          },
          {
            id: "s5",
            verb: "searching web",
            target: "competitor pricing page structure",
            status: "running",
          },
        ],
        text: "Both files are updated and use the shared config now",
        streaming: true,
        createdAt: Date.now() - 20000,
      },
    ],
  },
  { id: "c2", title: "Quarterly tax estimate", updatedAt: Date.now() - 3600_000, messages: [] },
  { id: "c3", title: "Trip to Lisbon", updatedAt: Date.now() - 86400_000 * 2, messages: [] },
  { id: "c4", title: "Summarize this contract", updatedAt: Date.now() - 86400_000 * 3, messages: [] },
  { id: "c5", title: "Resize batch of photos", updatedAt: Date.now() - 86400_000 * 4, messages: [] },
];
