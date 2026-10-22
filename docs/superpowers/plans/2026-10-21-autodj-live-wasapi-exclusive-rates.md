# AutoDJ Live WASAPI Official Exclusive Rates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Report which official AutoDJ Live sample rates—44.1, 48, and 96 kHz—an active WASAPI endpoint accepts in exclusive mode for the shared mix descriptor’s exact channel layout and sample representation.

**Architecture:** Extend the existing read-only probe by copying the complete `WAVEFORMATEX`/extended descriptor into aligned owned storage, changing only `nSamplesPerSec` and its derived `nAvgBytesPerSec`, and querying `IAudioClient::IsFormatSupported` without initializing a stream. Reuse the existing exact-format result for the shared rate, transport a sorted bounded list through the current protocol, and label its narrow meaning in Setup.

**Tech Stack:** Rust, windows-rs/Win32 WASAPI, MessagePack protocol, Tauri, TypeScript, Vitest.

**Spec:** `docs/superpowers/specs/2026-10-17-autodj-live-design.md`

## Global Constraints

- Query only the official sample rates `44_100`, `48_000`, and `96_000` Hz.
- Preserve every descriptor byte except `nSamplesPerSec` and the derived `nAvgBytesPerSec`; do not invent channel masks, valid-bit counts, subtypes, or sample representations.
- Probe only an active endpoint chosen explicitly by the DJ.
- Do not call `IAudioClient::Initialize`, start a stream, select a device, or claim measured latency.
- No new dependency and no ASIO capability claim.

## Review Focus

- Extended descriptor bytes and alignment remain valid when a candidate is passed to the driver.
- Byte-rate recalculation cannot overflow and always matches `sample_rate * block_align`.
- Unsupported candidates are omitted while unexpected HRESULTs fail the whole probe generically.
- The shared rate is not queried twice and agrees with `exclusive_shared_mix_supported`.
- UI wording cannot be read as support for other channel layouts, sample representations, buffer sizes, ASIO, or actual playback.

---

### Task 1: Build and probe official-rate candidates

**Files:**
- Modify: `crates/live-audio/src/discovery.rs`
- Modify: `crates/live-audio/src/discovery/windows.rs`
- Modify: `crates/live-protocol/src/lib.rs`
- Modify: `crates/live-protocol/tests/round_trip.rs`
- Modify: `crates/live-engine/src/server.rs`
- Modify: Rust fixtures constructing `WasapiCapabilities`

**Interfaces:**
- Consumes: the validated descriptor returned by `GetMixFormat` and the existing exact exclusive result.
- Produces: `exclusive_shared_format_sample_rates_hz: Vec<u32>` in audio and `Option<Vec<u32>>` in the protocol model so reattached older engines remain distinguishable from a probed empty result.

- [x] **Step 1: Write failing Rust tests**

Add tests proving a candidate changes only sample rate/byte rate while preserving extension bytes, overflow is rejected, supported rates are sorted/official/consistent with the exact result, and protocol round trips retain empty and populated lists.

- [x] **Step 2: Run targeted tests and verify missing-interface failures**

Run: `cargo test -p live-audio -p autodj-live-protocol`

Expected: FAIL because the candidate builder and capability field do not exist.

- [x] **Step 3: Implement the minimal candidate loop and transport mapping**

Copy the full descriptor into `usize`-aligned owned storage; mutate its header with unaligned reads/writes; reuse the exact result when the candidate rate equals the shared rate; query the other official rates with `AUDCLNT_SHAREMODE_EXCLUSIVE` and no closest-match pointer. Add the validated vector to audio/protocol structs, engine mapping, and fixtures.

- [x] **Step 4: Run targeted Rust verification**

Run: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`.

Expected: PASS.

- [x] **Step 5: Commit**

Commit message: `feat: probe official WASAPI exclusive rates`

### Task 2: Present bounded rate evidence and verify hardware

**Files:**
- Modify: `apps/autodj-live/src/setup.ts`
- Modify: `apps/autodj-live/src/setup.test.ts`
- Modify: `docs/autodj-live/development.md`
- Modify: `docs/superpowers/plans/2026-10-21-autodj-live-wasapi-exclusive-rates.md`

**Interfaces:**
- Consumes: camelCase `exclusiveSharedFormatSampleRatesHz: number[] | null`.
- Produces: explicit accepted-rate or none-accepted text scoped to the displayed channels/sample representation.

- [x] **Step 1: Write failing UI tests**

Assert populated and empty rate lists render distinct copy, preserve rate order, and retain the untested-layout/representation/ASIO limitation.

- [x] **Step 2: Run the focused UI test and verify it fails**

Run: `npm --prefix apps/autodj-live test -- --run src/setup.test.ts`

Expected: FAIL because Setup does not render the list.

- [x] **Step 3: Implement the label and update contributor documentation**

Add the TypeScript field and bounded label. Update the development guide’s stale shared-only paragraph to describe the exact descriptor plus official-rate variants and explicitly retain all remaining evidence limits.

- [x] **Step 4: Run the native probe and full verification**

Probe the connected active Rane endpoint without opening a stream. Run UI tests/checks/build, six workspace policy tests, architecture traceability, formatting, whitespace, and the full Rust workspace gate.

Expected: all automated gates pass; native output lists only accepted official rates for the preserved format template.

- [x] **Step 5: Review, commit, push, and prove synchronization**

Commit message: `feat: show official WASAPI exclusive rates`. Obtain one fresh whole-change review, defer Minor findings, fix Critical/Important findings test-first, push `feat/autodj-live-audio-routing`, and require local/remote divergence `0 0` without merging or opening a pull request.
