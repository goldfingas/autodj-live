# AutoDJ Live WASAPI Capability Probe Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a DJ explicitly probe an active Windows endpoint and see its WASAPI shared-engine mix format and timing without opening an audio stream.

**Architecture:** Extend the existing authenticated engine path with one endpoint-ID request and one sanitized response. The audio crate activates Windows `IAudioClient` only long enough to call `GetMixFormat` and `GetDevicePeriod`; it never initializes or starts a stream. Discovery and probing share the engine's single native-worker gate, and the Setup UI labels the result as shared-mode evidence only.

**Tech Stack:** Existing Rust workspace, pinned `windows` 0.62.2 bindings, authenticated MessagePack IPC, Tauri commands, vanilla TypeScript, Vitest.

**Spec:** `docs/superpowers/specs/2026-10-17-autodj-live-design.md` and `docs/autodj-live/audio-discovery.md`.

## Global Constraints

- Windows 11 24H2 and newer remains the first-release platform.
- Official product sample rates remain 44.1, 48, and 96 kHz; the probe reports the current shared mix even when it differs.
- A shared mix format is not ASIO capability, exclusive-mode support, a physical channel map, buffer-size support, or measured end-to-end latency.
- Probing is a separate explicit user action; discovery does not activate audio clients.
- No stream is initialized or started, no ASIO driver is loaded, and no endpoint or fallback is selected.
- Only the active authenticated supervisor owner may probe, and endpoint IDs never appear in visible copy or announcements.
- Push verified commits to the matching topic branch; no PR or master merge.

## Review Focus

- A stale, missing, disabled, unplugged, or non-audio endpoint ID must return a bounded failure without losing inventory.
- A stalled Windows call must remain single-flight and must not hold Tokio runtime shutdown.
- COM-allocated `WAVEFORMATEX` memory must be freed on every successful allocation path.
- Zero channels/rate, negative periods, and malformed format values must be rejected rather than displayed as capabilities.
- Repeated probes and refreshes must replace the endpoint's capability text without duplicating live regions or leaking endpoint IDs/errors.

---

### Task 1: Engine-owned WASAPI shared-format probe

**Files:**
- Modify: `crates/live-audio/src/discovery.rs`
- Modify: `crates/live-audio/src/discovery/windows.rs`
- Modify: `crates/live-protocol/src/lib.rs`
- Modify: `crates/live-protocol/tests/round_trip.rs`
- Modify: `crates/live-engine/src/lease.rs`
- Modify: `crates/live-engine/src/server.rs`
- Modify: `crates/live-supervisor/src/session.rs`
- Test: relevant colocated tests and supervisor integration tests

**Interfaces:**
- Produces: `WasapiCapabilities { endpoint_id, channels, sample_rate_hz, bits_per_sample, default_period_100ns, minimum_period_100ns }`
- Produces: `EngineCommand::ProbeWasapi { endpoint_id }`, `EngineEvent::WasapiCapabilities(...)`, and `LiveSession::probe_wasapi(endpoint_id: &str)`
- Consumes: the existing active-owner lease and single native audio-worker gate

- [x] Write protocol round-trip, lease authorization, format validation, partial failure, and real supervisor request tests; run them and observe missing-interface failures.
- [x] Add `live_audio::discovery::probe_wasapi(endpoint_id: &str)` on its own COM thread. Resolve the endpoint by ID, require active state, activate `IAudioClient`, read/free `GetMixFormat`, read `GetDevicePeriod`, validate nonzero format fields and nonnegative periods, and return no driver/COM internals on failure.
- [x] Make the engine worker gate generic and shared by discovery/probe. Authorize before dispatch and again before returning; keep timeouts bounded while a stalled worker remains single-flight outside Tokio ownership.
- [x] Add the protocol mapping and supervisor method. Verify no stream initialization APIs are called and the existing discovery behavior remains unchanged.
- [x] Run focused Rust tests, formatting, and Clippy; commit and push the engine checkpoint.

### Task 2: Accessible explicit probe in Setup

**Files:**
- Modify: `apps/autodj-live/src-tauri/src/lib.rs`
- Modify: `apps/autodj-live/src-tauri/tests/app_lifecycle.rs`
- Modify: `apps/autodj-live/src-tauri/tests/production_supervisor.rs`
- Modify: `apps/autodj-live/src/setup.ts`
- Modify: `apps/autodj-live/src/setup.test.ts`
- Modify: `apps/autodj-live/src/main.ts`
- Modify: `apps/autodj-live/src/styles.css`
- Modify: `docs/autodj-live/audio-discovery.md`
- Modify: `docs/autodj-live/development.md`

**Interfaces:**
- Consumes: `LiveSession::probe_wasapi(endpoint_id: &str) -> Result<WasapiCapabilities, SupervisorError>`
- Produces: Tauri `probe_wasapi_endpoint(endpoint_id: String)` and UI `AudioSetupApi.probe(endpointId)`

- [x] Write Tauri and DOM tests first. Assert no automatic probe; only active endpoints get a Probe button; the correct hidden ID is requested; loading is bounded to the activated action; shared format/timing replaces prior text; refresh removes stale results; failure copy is generic; announcements contain no IDs.
- [x] Add lifecycle/backend pass-through and the Tauri command, returning only the capability DTO.
- [x] Render one endpoint-local action/result. Label output `WASAPI shared mix` and state `Exclusive and ASIO support untested`; describe periods as engine periods, not latency.
- [x] Run UI tests/lint, Tauri tests, full Rust workspace tests, architecture policy, formatting, and whitespace checks.
- [x] Exercise a connected active endpoint through the native probe path without initializing a stream; record exact output and limitations. Obtain independent review, fix Important findings test-first, commit/push, and verify local/remote `0 0`.

## Completion checkpoint

- Native evidence: `Mic 1/2 (Rane PERFORMER)` reported a 2-channel, 48 kHz, 32-bit shared mix with 10 ms default and 3 ms minimum Windows engine periods; no stream initialization/start call exists.
- Automated verification: 68 Rust workspace tests and 15 UI tests passed; Clippy with warnings denied and the TypeScript/Vite production build passed.
- Policy verification: six Live workspace policy tests, architecture traceability, formatting, and whitespace checks passed.
- Independent review: the Important malformed-`WAVEFORMATEX` finding was fixed RED→GREEN by validating format tag, extension size, channel/sample width, block alignment, and byte rate before extraction.
- Evidence limit: physical latency, exclusive mode, ASIO capability, physical channel mapping, and actual NVDA speech remain unverified follow-ons.

## Explicit follow-ons

WASAPI exclusive-mode `IsFormatSupported`, low-period/event-driven stream setup, ASIO SDK integration, ASIO driver capability probing, physical channel naming, route editing, persistence, and measured latency remain later work.
