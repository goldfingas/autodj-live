# AutoDJ Live Setup Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show the engine-owned, read-only Windows audio inventory in AutoDJ Live Setup.

**Architecture:** Add sanitized discovery DTOs and one request/response pair to the authenticated engine protocol. The engine invokes `live-audio` discovery outside its async runtime, the supervisor transports the result, and Tauri exposes it to a small accessible Setup surface. Nothing selects, opens, probes, saves, or polls a device.

**Tech Stack:** Existing Rust workspace, authenticated MessagePack IPC, Tauri commands, vanilla TypeScript, Vitest.

**Spec:** `docs/autodj-live/audio-discovery.md` and `docs/superpowers/specs/2026-10-17-autodj-live-design.md`.

## Global Constraints

- Audio discovery remains engine-owned; WebView and Python never access Windows audio APIs.
- Every ASIO entry visibly says registered/unprobed; endpoints show direction and availability.
- IDs remain distinct identity, but the UI does not expose credentials, ports, or process IDs.
- Discovery is user-triggered; no automatic polling, device selection, driver loading, stream opening, persistence, or fallback.
- A discovery issue preserves healthy entries and is visible as text; only the Refresh result is announced.
- Push verified commits to the matching topic branch; no PR or master merge.

## Review Focus

- Discovery must require the active authenticated supervisor owner.
- Blocking Windows enumeration must not stall the engine's current-thread Tokio loop.
- Partial issues must not discard healthy devices.
- Duplicate display names must render as distinct entries without using names as keys.
- Repeated refreshes must replace the list rather than append duplicates or create noisy live regions.

### Task 1: Engine discovery request

**Files:** protocol DTOs/tests, engine dependencies/server/lease tests, supervisor session API/tests.

**Interfaces:** `EngineCommand::DiscoverAudio`; `EngineEvent::AudioDiscovery(AudioDiscovery)`; DTOs `AudioEndpoint`, `AsioRegistration`, enums `AudioDirection`, `AudioEndpointState`, and `AudioDiscovery { endpoints, asio_drivers, issues }`; `LiveSession::audio_discovery()`.

- [x] Write wire round-trip tests and lease authorization tests; observe failure for missing types/variant.
- [x] Add protocol DTOs with stable camel-free Rust wire fields and no secrets/process/transport data.
- [x] Authorize discovery only for the active owner in `Lease`; return a private dispatch marker, then run `live_audio::discovery::discover` outside the runtime in the server and map to protocol DTOs.
- [x] Add a supervisor request method and an integration test against the real test engine; verify discovery returns without opening streams.
- [x] Run focused Rust tests, format and Clippy; commit and push.

### Task 2: Accessible Setup inventory

**Files:** Tauri lifecycle/command/tests, `src/setup.ts`, `src/setup.test.ts`, `main.ts`, styles, package test script, development docs.

**Interfaces:** Tauri `get_audio_discovery() -> Result<AudioDiscovery, ()>`; UI `mountAudioSetup(root, { discover })`.

- [x] Write Tauri boundary and DOM tests first. Assert an `h2` Setup heading; Refresh button; endpoint name/direction/state; ASIO registered/unprobed label; issue text; duplicate names as separate list items; replacement on refresh; one polite summary announcement; loading and failure text; no automatic request.
- [x] Add the backend/lifecycle pass-through and Tauri command, returning only protocol DTO fields.
- [x] Implement the Setup module and minimal club-readable styles. Disable Refresh only while pending and restore keyboard focus afterward.
- [x] Run Live UI tests/lint, Tauri tests, full Rust workspace tests, architecture and whitespace checks.
- [x] Build and launch the command path without selecting/opening hardware, record the unavailable UI-automation/NVDA check, obtain independent review, commit/push, and verify local/remote `0 0`.

## Completion checkpoint

- UI: 12 Vitest tests passed; TypeScript and Vite production build passed.
- Rust: 64 workspace tests passed; workspace Clippy passed with warnings denied.
- Policy: six Live workspace tests, architecture traceability, formatting, and whitespace checks passed.
- Review fixes: discovery is single-flight on a native worker that cannot hold Tokio shutdown; Refresh completion preserves focus when the DJ moves elsewhere.
- Manual limit: the built app launched with real engine-owned discovery, but visual/NVDA click-through was not possible because the Windows automation connector exposed no usable app/window methods.

## Explicit follow-ons

Capability negotiation, ASIO SDK integration, WASAPI modes, channel probing,
route editing, persistence, device-loss handling, and playback remain later work.
