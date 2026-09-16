## 1. Codex Provider

- [x] 1.1 Add `DataSource::Codex`, export the new provider module, and define the Codex app-server/account/rate-limit serde models; verify the Rust model serializes the source as `codex` and `cargo test --manifest-path src-tauri/Cargo.toml` compiles the new types.
- [x] 1.2 Implement GUI-safe Codex binary discovery and a bounded `codex app-server --stdio` JSON-RPC session with initialize handshake, response-id correlation, output size limits, timeout, child cleanup, and secret-free diagnostics; verify a locally installed Codex is discovered when launched with a minimal GUI-like PATH and timeout/error paths terminate the child without hanging.
- [x] 1.3 Implement account-mode and RPC-error classification for missing CLI, unsupported method, signed-out, API-key-only, relogin, permission, and transient failures; verify each observable condition returns the status and actionable non-secret message required by `codex-usage/spec.md` using controlled provider inputs.
- [x] 1.4 Normalize multi-bucket rate limits with single-bucket fallback, stable ordering, primary/secondary windows, percentage totals, duration labels, reset timestamps, and `plan_tier`; verify representative controlled responses produce no duplicate default bucket and omit absent optional windows instead of fabricating zero values.

## 2. Application Integration

- [x] 2.1 Add Codex to `refresh_all` as the fourth concurrent provider and merge it through the existing snapshot path; verify manual refresh returns a Codex snapshot and an `Ok` Codex window at 80% participates in the existing tray warning and accelerated-polling calculation.
- [x] 2.2 Mirror the `codex` source in `src/App.tsx`, map its display name to “Codex”, and reuse the generic plan/window/status rendering without credential settings; verify `npm run build` succeeds and the panel renders Codex windows and actionable unavailable states.

## 3. Diagnostics and Documentation

- [x] 3.1 Extend `src-tauri/examples/probe.rs` with an explicitly non-CI Codex probe that reports status, normalized window metadata, and timing without printing account responses or credentials; verify `cargo run --example probe --manifest-path src-tauri/Cargo.toml` succeeds with a ChatGPT-authenticated Codex CLI.
- [x] 3.2 Update `README.md` with Codex support, ChatGPT-login and compatible-CLI prerequisites, unsupported API-key billing scope, credential ownership, and relogin/upgrade guidance; verify the documented commands and behavior match the implemented UI and provider states.

## 4. Validation

- [x] 4.1 Run `cargo test --manifest-path src-tauri/Cargo.toml` and verify all Rust tests pass without contacting live Codex services.
- [x] 4.2 Run `npm run build` and verify the TypeScript mirror and production frontend build complete successfully.
- [x] 4.3 Run the live probe manually with real local Codex credentials, verify the expected limit buckets, percentages, reset times, plan and refresh latency against Codex’s own usage view, and record that this check is not CI-safe.
