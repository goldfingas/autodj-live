# 14. Separate AutoDJ Live real-time ownership from the desktop shell and Python

Date: 2026-10-17
Status: Proposed; explicit maintainer acceptance pending.
Decision authority: Project maintainer; approval of the product design does not
by itself accept this ADR or establish implementation evidence.

## Context and scope

The approved [AutoDJ Live design](../superpowers/specs/2026-10-17-autodj-live-design.md)
adds a Windows desktop performance application with two decks, local controller
handling, and AutoDJ assistance. Audio must survive a WebView reload or shell
crash, and Python analysis or library work must not block live rendering. This
requires a separate real-time owner, authenticated local control, and explicit
reconnection and compatibility rules.

This record proposes the ownership and failure boundaries. The foundation
establishes process lifecycle, protocol, supervision, and a status UI; it does
not implement audio, DSP, decks, MIDI, ASIO, or stems. Later stages must retain
these boundaries and supply their own hardware and recovery evidence. Product
requirements originate in the approved conversation/design; no GitHub issue or
PR is linked yet. Publish those requirements with an authorized issue or PR
before contributor handoff, following [CONTRIBUTING](../../CONTRIBUTING.md).

## Relationship to existing decisions

- Accepted [ADR 3](0003-browser-driven-playback.md) retains browser playback as
  the existing AutoDJ default. Live adds a separate application and does not
  move that browser's audio ownership.
- Accepted [ADR 4](0004-server-mix-bus-and-stream-mode.md) retains the existing
  station server mix bus and output paths. Live's native engine owns its mix;
  future streaming/recording integration uses a bounded handoff, not a second
  independently timed live mix.
- Accepted [ADR 5](0005-a-station-owns-its-playout-chain.md) retains station
  settings, state, and playout ownership with a shared catalog. Live session
  state, routes, and mappings remain separate from station runtime state.
- Proposed [ADR 8](0008-bound-background-work-outside-playout.md) describes
  bounded background work and resource cleanup. Live adds a distinct engine
  lease; existing station child cleanup must not kill that engine on shell
  failure. Worker tests do not prove Live continuity or timing.
- Proposed [ADR 10](0010-preserve-state-through-atomic-writes-and-recovery.md)
  describes validated atomic writes, secret separation, and recovery limits.
  Live needs its own journal and protected discovery record; existing file
  tests do not establish Windows privacy or actual power-loss durability.
- Proposed [ADR 11](0011-verify-accessibility-with-browser-and-speech-evidence.md)
  supplies interaction and speech evidence principles. Live's WebView requires
  its own keyboard, focus, and NVDA samples; existing browser samples do not
  certify the desktop application.
- Proposed [ADR 12](0012-separate-runtime-setup-from-model-execution.md)
  describes dependency isolation and explicit runtime setup. Live adds separate
  desktop artifacts while preserving the existing Python installation and
  launcher. No startup path may silently install missing prerequisites.

This Proposed record supersedes none of those decisions and does not change
their acceptance status.

## Proposed decision

### Process ownership and control

Use a separately supervised, long-lived Rust engine process for deck transport,
decode buffers, DSP, scratching, sampler voices, audio/controller I/O, clocking,
and real-time state. Tauri owns the window, restricted control commands, child
startup, reconnection, and normal shutdown. The WebView displays state and
requests actions; neither it nor Python owns club-critical rendering or timing.

Python retains catalog, metadata, analysis, automation planning, migrations, and
stream/record coordination through a bundled local sidecar. It sends complete
timestamped transition plans with the expected deck-ownership generation. Rust
validates and schedules those plans, owns their curves, and rejects stale
session or ownership generations. High-frequency deck/meter data does not
round-trip through Python. The callback performs no file/network I/O, logging,
allocation, inference, database work, enumeration, or unbounded locking.

Engine and sidecar control listeners bind only to `127.0.0.1` on ephemeral
ports. Require a fresh 256-bit per-launch bearer token before returning state,
including health/process information. Forged or stale tokens receive no such
information. Tokens travel through protected child environment/runtime state,
never command-line arguments, startup descriptors, logs, crash reports, or UI
text. The WebView receives restricted shell commands, not a reusable credential
or an unauthenticated LAN control endpoint.

### Orphan lease, persistence, and recovery

The engine survives a shell crash for a 30-second reconnection lease. While
orphaned it starts no new automation; already loaded decks and local controller
handling remain engine-owned. The foundation has no audio to demonstrate this
continuity. If the lease expires, the engine exits safely. Normal app exit
explicitly shuts down owned children instead of waiting for lease expiry.

A replacement shell authenticates and reattaches to the matching live session
and engine generation. A second launch must not create a duplicate real-time
owner while the lease is active. Sidecar failure removes automation capability
without stopping already loaded decks; restart/resynchronization is explicit.

Keep discovery credentials separate from ordinary session exports and support
bundles. The installed Windows path uses DPAPI for secret protection; a portable
fallback fails closed if privacy cannot be established. Validate the discovery
record and live identity before reattachment; stale records are not authority
to attach to a different process.

Later audio stages journal bounded engine snapshots outside the callback using
validated atomic replacement. Recovery after an unclean engine shutdown
restores the session paused and requires route confirmation and explicit resume
before emitting audio. This persistent recovery is distinct from reattaching to
a still-running engine during its lease. Journal and real power-loss evidence
remain gaps until their owning delivery stages land.

### Compatibility and distribution

Existing AutoDJ browser/server workflows remain installable and runnable without
Rust, Tauri, ASIO, or MIDI components. Keep Live tooling and artifacts isolated;
the existing `autodj serve` path retains its behavior. Live ships version-matched
shell, engine, and sidecar artifacts; startup reports missing prerequisites
without installing them.

Use a versioned shared engine protocol, initially v1.0. Reject incompatible
major versions before accepting control or reporting session state; never guess
compatibility from a successful connection. Session and generation validation
remain required after protocol compatibility succeeds.

Shared-catalog changes require explicit migrations, transactions, and ownership
locks. Every migration declares mixed-version compatibility and refuses unsafe
concurrent writes. This foundation introduces no catalog or station migration.

## Alternatives and consequences

- An engine inside the Tauri process would simplify calls but would lose the
  audio owner on a shell crash.
- Browser or Python-owned performance audio could reuse more playback code,
  but cannot meet the separate ownership and callback constraints of this design.
- A separate engine process provides the required failure boundary, at the cost
  of authenticated IPC, session discovery, bounded leases, and duplicate-owner
  prevention. This is the proposed choice.

Process separation alone cannot guarantee audio continuity or latency. IPC and
discovery failures need bounded handling; packaging must keep three components
compatible. Revisit this decision if the real-time owner, supported platforms,
trust boundary, lease policy, or mixed-version storage policy changes. Moving
club-critical work into the WebView or Python requires a new product decision.

## Implementation and verification map

Planned foundation entrypoints are `crates/live-protocol/` (Task 3),
`crates/live-engine/` (Task 4), `src/autodj/live_sidecar.py` and
`src/autodj/live_sidecar_cli.py` (Task 5), `crates/live-supervisor/` (Task 6),
and `apps/autodj-live/` (Task 7). Task 2 owns workspace/dependency isolation;
Task 8 owns CI, development guidance, and the foundation evidence record.
These are planned paths, not implemented or verified evidence. Later delivery
stages in the [design](../superpowers/specs/2026-10-17-autodj-live-design.md#delivery-stages)
own audio, journals, catalog concurrency, hardware, and release validation.

Use the existing [architecture checker](../../scripts/check_architecture_docs.py)
and [checker tests](../../tests/unit/test_architecture_docs.py) for structural
validation. They do not execute Live behavior or accept this decision. As each
owning task lands, update its evidence with concrete tests and reciprocal IDs
where supported by the checker; preserve manual and unimplemented gaps.

## Current contracts

IDs remain stable. No Live implementation evidence is established by this
documentation task. A manual procedure below is an unfulfilled obligation, not
a claim that the procedure has run or passed.

| ID | Constraint | Method | Evidence |
| --- | --- | --- | --- |
| `ADR-0014-01` | A separate Rust engine owns real-time audio/controller state; WebView reload or shell failure does not transfer that ownership to the UI or Python. | gap | Tasks 4, 6, and 7 must establish process identity and lifecycle tests. Later audio stages must verify loaded-deck/controller continuity and callback restrictions; no implementation evidence yet. |
| `ADR-0014-02` | Engine and sidecar control bind only to `127.0.0.1` on ephemeral ports and require a fresh 256-bit per-launch bearer token before revealing state; secrets stay out of arguments, descriptors, logs, crash reports, and UI. | gap | Tasks 3 through 6 must establish authentication, rejection, redaction, and protected discovery tests; Windows secret protection also needs platform evidence. No implementation evidence yet. |
| `ADR-0014-03` | A shell crash permits a 30-second engine reconnection lease with no new automation while orphaned; matching sessions reattach without a duplicate owner, and lease expiry exits safely. | gap | Tasks 4 and 6 must establish lease, expiry, generation, and duplicate-owner tests. Persistent recovery later requires validated atomic journals and paused restore with route confirmation; neither lifecycle nor journal implementation evidence exists yet. |
| `ADR-0014-04` | Existing AutoDJ installs and runs without Rust, Tauri, ASIO, or MIDI components; Live startup never silently installs prerequisites. | gap | Tasks 2 and 5 must establish workspace, launcher, and import-boundary checks; Task 8 must record applicable packaging/CI results. No implementation evidence yet. |
| `ADR-0014-05` | The shared engine protocol rejects incompatible major versions before accepting control or returning session state; session and ownership generations reject stale commands. | gap | Task 3 owns protocol-version rejection tests; Tasks 4 and 6 own session/reattachment checks. Later deck stages own automation-generation behavior. No implementation evidence yet. |
| `ADR-0014-06` | Windows process/restart, protected discovery, and actual runtime behavior require manual platform evidence beyond structural or fake-backend checks. | manual | On Windows 11 24H2 or newer, launch Live, record engine/session identity, reload the WebView, terminate and restart the shell within 30 seconds, verify matching reattachment and no duplicate engine, then repeat without restart to verify safe lease expiry. Exercise sidecar failure and normal shutdown; inspect discovery privacy and token redaction. Record exact build, Windows version, timings, commands, results, and limitations in Task 8's evidence record. Not yet performed; later audio/hardware and NVDA/release evidence remain separate obligations. |
