# AutoDJ Live design

Date: 2026-10-17
Status: Draft for maintainer approval
Product name: **AutoDJ Live**

## Intent

AutoDJ Live is a Windows desktop application for DJs who need hands-on club
performance and AutoDJ assistance in the same reliable system. It is not a
desktop wrapper around the existing browser player. Its club-critical audio,
controller, timing, and recovery paths run in a native real-time engine, while
it reuses AutoDJ's catalog, analysis, automation, streaming, and accessibility
work through a bundled local service.

The first release is designed around a two-deck club workflow. A DJ can mix
manually, arm AutoDJ to load and mix either deck, take control of a deck by
touching its controls, and hand it back without an abrupt transport or mixer
change. The application must remain useful without a network connection or an
account.

## Product principles

- **Audio continuity is more important than UI continuity.** UI stalls,
  analysis work, updates, logging, and library operations must not block the
  real-time callback.
- **Automation never fights the DJ.** AutoDJ moves a deck only while armed.
  Direct keyboard, pointer, or controller interaction takes ownership through
  an explicit and observable state transition.
- **Hardware behavior is inspectable.** Auto-detection reports which mapping
  was loaded, and every input and output message can be monitored and edited.
- **Offline is the normal case.** Playback, mapping, recovery, analysis, and
  licensing notices work locally. Streaming and update checks are optional.
- **Accessibility is a release requirement.** Every action is keyboard
  addressable; focus, announcements, high contrast, reduced motion, and screen
  reader workflows are tested rather than inferred.
- **Existing AutoDJ remains supported.** The browser/server application does
  not require Tauri, Rust, ASIO, or MIDI components to install or run.

## Goals

1. Provide two responsive performance decks with cueing, looping, beat sync,
   slip, scratching, motorized-platter support, hot cues, phrase markers, and
   complete mixer control.
2. Provide MIDI 1.0 and MIDI 2.0 input and output, MIDI Clock, song position,
   MIDI Time Code, hot-plugging, multiple simultaneous devices, automatic
   profile matching, guided learn, and an expert mapping editor.
3. Provide ASIO and WASAPI audio backends, multichannel input and output,
   internal mixing, master/booth/cue/record buses, per-deck direct outputs, and
   controlled bridges to secondary audio interfaces.
4. Provide a clean club performance view, a separate setup workspace, and one
   unified action registry for UI, keyboard, MIDI, automation, and tests.
5. Share AutoDJ's local catalog and analysis cache safely, import common DJ
   library metadata without modifying third-party databases, and preserve the
   current AutoDJ application's behavior.
6. Recover predictably from application, controller, and device failure while
   keeping enough local evidence to diagnose problems without sending
   telemetry automatically.

## First-release exclusions

- macOS and Linux builds
- control-vinyl/CD timecode (DVS)
- commercial streaming music catalogs
- third-party VST3 or other plugin hosting
- cloud accounts, cloud mapping sync, or required online activation
- writing into Rekordbox, Serato, Traktor, or Mixxx databases
- video mixing, lighting control, and karaoke features
- simultaneous independent ASIO drivers as real-time clock masters

These exclusions do not prevent later ADRs or product decisions from adding
the capabilities.

## Relationship to AutoDJ

AutoDJ Live lives in the existing repository and is licensed under AGPLv3.
The repository becomes a monorepo with three deliberately separated surfaces:

1. The existing Python package remains the owner of the music catalog,
   analysis jobs, automation rules, library metadata, station behavior,
   recording/stream integration, and compatibility migrations.
2. A new Rust live engine owns real-time deck transports, decode buffers, DSP,
   scratching, sampler voices, controller I/O, audio-device I/O, clocking, and
   the real-time-safe state snapshot consumed by the UI.
3. A new Tauri application owns the AutoDJ Live window, lifecycle, secure local
   service startup, setup/performance workspaces, and signed desktop updates.

The Live application starts a version-matched Python sidecar on loopback using
an authenticated, per-launch channel. It must not expose a new unauthenticated
LAN control surface. The sidecar provides catalog, search, metadata, analysis,
automation, and stream/record coordination. High-frequency deck and meter data
does not round-trip through Python. Python sends a complete, timestamped
transition plan with its expected deck-ownership generation. Rust validates
and schedules that plan and owns every fader, EQ, sync, transport, effect, and
cancellation curve used to render it. Python does not stream individual
time-critical control changes throughout a transition.

AutoDJ and AutoDJ Live may run concurrently against one catalog. Catalog and
analysis changes use explicit migrations, transactions, and ownership locks.
Live session state, mappings, audio routes, and UI preferences are separate
from station runtime state. A schema migration must declare whether both the
old AutoDJ version and the new Live version may remain open during migration;
unsafe mixed-version writes are refused rather than guessed.

[Proposed ADR 14](../../adr/0014-separate-autodj-live-realtime-ownership.md)
records the audio ownership, distribution, concurrency, and dependency
boundaries and reconciles ADRs 3, 4, 5, 8, 10, 11, and 12. Its acceptance
remains a separate maintainer decision from approval of this product design.

## Desktop and process architecture

### Tauri shell

The Tauri shell supplies the signed Windows application, WebView-hosted UI,
window and power events, update orchestration, and restricted commands into
the live engine. The UI uses web technology so existing AutoDJ modules and
accessibility patterns can be reused where their ownership fits. Club audio
must continue when the WebView is reloaded or its render process fails.

### Rust live engine

The engine is a long-lived, separately supervised native process rather than
an in-process Tauri library. Audio and controller handling therefore survive a
WebView reload and a Tauri shell crash. The engine keeps running for a bounded
reconnection window and then follows the configured safe unattended behavior;
it never starts new automation while its supervisors are unavailable. The
engine has these internal boundaries:

- **audio host:** ASIO and WASAPI device discovery, capability negotiation,
  stream lifecycle, sample-format conversion, and underrun reporting;
- **graph:** fixed-allocation real-time graph for decks, sampler, inputs,
  effects, buses, cue, direct outs, meters, recording taps, and bridges;
- **decks:** decode/preload buffers, playhead, tempo/key controls, cue/loop
  state, slip state, beat grid, synchronization, ownership, and scratch model;
- **controller service:** Windows MIDI Services when available, legacy MIDI
  fallback, endpoint identity, MIDI 1/2 translation, feedback scheduling,
  mapping evaluation, clock/MTC, and hot-plug lifecycle;
- **action service:** typed command registry shared by UI, keyboard, MIDI,
  automation, accessibility descriptions, guard rules, and test fixtures;
- **state journal:** non-real-time persistence of bounded state snapshots;
- **diagnostics:** lock-free counters and bounded event queues drained outside
  the callback.

The real-time callback performs no file I/O, network I/O, logging, allocation,
model inference, database work, device enumeration, or unbounded locking.
Track decoding, waveform generation, stem preparation, library import, profile
validation, and route changes are prepared outside the callback and committed
at safe graph boundaries.

### Python sidecar

The sidecar reuses current AutoDJ services rather than duplicating library and
automation logic in Rust. It owns catalog queries, indexing, existing cue and
DJ metadata imports, track selection, scheduled automation, offline analysis,
stream/relay coordination, and shared database migrations. It may request
typed engine actions but cannot write directly into engine memory.

The sidecar is supervised by Tauri. Failure removes automation capability but
must not stop already loaded decks, local controller handling, or master/cue
audio. Restart and resynchronization are explicit; stale automation commands
are rejected by session and ownership generation.

## Audio system

### Supported formats and timing

- Official sample rates: 44.1, 48, and 96 kHz.
- Internal processing: planar or interleaved float32 selected once per graph,
  with no callback-time format negotiation.
- Target: stable 128-sample operation on supported ASIO hardware, roughly
  5-10 ms total interactive latency. Larger buffers remain valid fallbacks.
- Resampling occurs at controlled boundaries with quality appropriate to music
  playback. It is measured separately from scratch-control latency.
- Performance claims require measurements on named machines, drivers, sample
  rates, buffer sizes, and channel layouts. Unit tests do not certify latency.

### Backends and devices

ASIO is the preferred professional backend. The application hosts drivers the
user installed for their hardware; it does not redistribute vendor drivers.
The ASIO SDK integration uses its GPLv3-compatible option, keeps notices and
corresponding source, and remains isolated enough for its obligations to be
audited. WASAPI supports shared low-period and exclusive event-driven modes and
is the compatibility and emergency fallback.

One device is the real-time clock master. Its channels may be mapped to deck
direct outs, master, booth, cue, sampler, record, stream, or input strips.
Secondary WASAPI interfaces use bounded asynchronous bridges with measured
drift, resampling, fill targets, and an explicit additional-latency warning. A
secondary bridge is not presented as sample-locked club monitoring. Native
Dante, AES67, AVB, or other network-audio protocol implementations are not in
the first release; network devices are usable only when a supported Windows
driver exposes them through ASIO or WASAPI.

Loss of the primary device safely mutes affected outputs while keeping deck
clocks, loaded buffers, controller state, and the journal alive. If a fallback
route was prepared, the UI offers it with its changed channel layout and
latency; live audio is not rerouted without confirmation.

### Mixer and routing

The internal mixer has two full decks, sixteen sampler slots in two banks, and
named hardware-input strips. Each input strip supports gain, trim/metering,
EQ, cue, mute, talkover/ducking where applicable, recording, and routing to
master, booth, stream, or other supported buses. Decks can use the internal
mixer and also expose pre- or post-fader direct outs for an external club
mixer. Feedback is prevented by explicit route validation.

Master, booth, cue, recording, stream, and per-deck direct paths are separate
logical buses even when they share hardware channels. Route changes are
prepared, validated against device capabilities, previewed in the UI, and
atomically swapped. Output meters distinguish signal level from device health.

Recording captures the master and optional stems/buses without blocking the
callback. Existing AutoDJ streaming and relay behavior is reused through a
bounded handoff from the Live engine rather than a second independent live mix.

## Decks, scratching, and automation ownership

Both decks support transport, tempo and key control, sync, quantize, slip,
hot cues, saved loops, beat jump, slicer, pad modes, and editable beat/phrase
grids. The display uses aligned scrolling RGB waveforms with overview strips,
zoom, playhead, phase, cues, loops, and phrase markers.

Jog input distinguishes touch, movement, release, edge nudging, and platter
motor feedback. Mapping profiles describe resolution, relative encoding,
physical range, nominal rotation speed, direction, touch semantics, and output
feedback. Scratch audio uses a continuous playhead/velocity model with bounded
interpolation and a tunable device curve. Motorized support adds torque/start-
stop state and platter-position feedback without assuming every device exposes
the same protocol. Unsupported vendor-only display or HID behavior is reported
as unavailable rather than emulated unreliably.

Each deck has an ownership state: `manual`, `automation-armed`,
`automation-active`, or `handoff`. Direct user action on an active deck freezes
the automated transition at its current audible transport, fader, EQ, and
effect values, cancels the unrendered remainder of the plan, and moves the deck
to manual ownership immediately. Automation does not finish the old transition
and does not retain control of untouched parameters. AutoDJ commands carry the
expected ownership generation and cannot act on a deck after the DJ takes it.
Handing a deck back requires a newly generated plan and shows the exact queued
action before arming it.

## Effects, sampler, and stems

The first release includes a finite full-performance baseline: per-deck
three-band EQ, gain and filter; echo/delay, reverb, flanger, phaser, gater,
bitcrusher, beat roll/repeat, pitch, brake, backspin, slip, quantization, pad FX,
slicer, and stem level/isolation controls. A rack chains at most four effects
per deck or sampler bus, supports tempo-synchronized and free-time parameters,
wet/dry control, ordered bypass, saved presets, and bounded LFO/envelope
modulation. Effects expose bounded parameters through the common action
registry and define automation smoothing. User racks cannot introduce
third-party executable code. More effects or deeper rack graphs are additions,
not implied release blockers.

The sampler has sixteen slots in two eight-pad banks. Slots support one-shot or
loop behavior, quantization, sync, choke groups, cueing, effects, gain, color,
saved assignment, and routable output.

Vocals, drums, bass, and instrumental stems are generated before performance
and cached locally. Analysis always has a supported CPU path and may use
DirectML/CUDA acceleration when available. The signed CPU-capable model and its
required license notices are included in every installer and portable package;
first use requires no download. Model identity, input version, output layout,
and source-track identity are recorded so stale stems are never silently
reused. Stem inference cannot run on the real-time path and is throttled or
paused automatically in performance mode.

## MIDI and controller system

### Protocol coverage

The controller service supports MIDI 1.0 and MIDI 2.0/UMP input and output,
notes, control changes, pitch bend, channel and poly pressure, program changes,
7-bit and 14-bit values, NRPN/RPN, relative encoders, SysEx, per-note controls,
endpoint metadata, scheduled output, and profile negotiation where the OS and
device expose them. It also sends or follows MIDI Clock, start/stop/continue,
song position, and MTC with explicit master/follower selection and smoothing.

On Windows 11 25H2 or newer, the application detects and uses Windows MIDI
Services when the system API is available. On 24H2, or when the service cannot
start, it falls back to a legacy MIDI 1.0 API and clearly reports the reduced
capabilities. Preview Windows MIDI runtime files are not bundled in a stable
release unless their then-current go-live terms are reviewed and satisfied.

Multiple controllers can be active simultaneously. Each endpoint has a stable
identity record and independent input/output profile. Action priority is
deterministic across device, layer, context, and modifier; conflicting bindings
are shown before save.

### Detection and mapping

Profile matching uses stable endpoint properties, USB vendor/product identity
when available, protocol version, port topology, and an explicit compatibility
range. Connecting a recognized device loads its last compatible profile,
notifies the DJ, and offers undo or profile selection. Unknown devices open
guided learn without blocking use as a generic MIDI source.

Mappings are human-readable, versioned, validated local files. They support
backup, schema migration, import/export, and optional signed curated updates.
They contain no arbitrary executable scripts. A profile can describe input
transforms, dead zones, acceleration curves, high-resolution pairs, layers,
modifiers, press/hold/release behavior, LED/color/display feedback, SysEx/UMP
templates, motor characteristics, and capability flags.

The editor has two synchronized views:

- guided learn: choose an action, move a control, inspect the detected message,
  set the behavior, test feedback, and save;
- advanced table: search and edit raw messages, ranges, curves, contexts,
  modifiers, layers, feedback, colors, and capability conditions while a
  bounded live monitor shows traffic.

The keyboard shortcut editor uses the same action definitions and conflict
engine. Every product action is bindable. Actions such as quit, stop-all,
ejecting a playing deck, or replacing a loaded track carry guard metadata and
may require a hold, double press, modifier layer, or confirmation.

## Performance and setup interface

The main view is a fixed club-performance workspace, not a freely dockable
dashboard. It keeps two decks and aligned waveforms prominent, places the
mixer between them, and keeps library/queue access persistent. Critical state
such as deck ownership, sync master, audio health, active controller profiles,
recording, stream state, and fallback readiness is always visible without
opening settings.

Setup is a separate workspace for audio devices and routes, MIDI devices and
mappings, keyboard shortcuts, library import, analysis/stems, recording,
streaming, updates, and diagnostics. Setup changes that could interrupt sound
are staged and applied explicitly.

The interface follows AutoDJ's announcement and focus principles: native
controls where practical, visible focus, one announcement path per event,
quiet meters/timers, reduced motion, forced-colors support, scalable high-
contrast themes, and short action-oriented labels. Waveforms expose navigable
textual cue, phrase, grid, and timing information. No essential operation
requires a pointer or visual waveform interpretation.

## Library and third-party import

The shared AutoDJ catalog remains the source of truth. Live adds crates,
playlists, performance history, saved loops, beat-grid corrections, mapping
references, stem cache identity, and non-destructive import provenance through
versioned schemas with declared ownership.

Importers read documented official export or backup formats from Rekordbox,
Serato, Traktor, and Mixxx to bring in playlists/crates, cues, loops, ratings,
tags, beat grids, and compatible analysis. Private live databases are not
parsed, even read-only, in the first release. Importers never modify the source
artifact. Every import reports unsupported or lossy fields, records provenance,
and can be rerun idempotently. Network shares are not a first-release
guarantee; missing tracks remain visible and repairable without blocking the
rest of the library.

Live plays every codec/container combination accepted by the AutoDJ version it
ships with. Decode behavior, duration, seekability, and metadata must remain
compatible across both products. The Rust engine may use a native decoder or a
bounded FFmpeg-backed worker, but unsupported divergence is a defect and codec
licenses must be included in distribution review.

## Persistence, recovery, and diagnostics

The engine journals bounded state snapshots outside the callback: loaded track
identity, play positions, loops/cues, tempo, ownership, mixer and route state,
sampler assignments, queue/automation reference, controller profiles, and the
last clean engine generation. Writes use atomic replacement and validation.

After an unclean shutdown, Live shows what was recovered and restores the
session paused. It does not emit audio until the DJ confirms routes and resumes.
Secrets and credentials are stored separately and excluded from ordinary
profile exports and support bundles.

Structured logs, underrun counters, device changes, mapping decisions, and
bounded performance traces remain local. A support-bundle tool redacts paths,
identifiers, credentials, track metadata, and other personal data, shows the
user exactly what will be included, and sends nothing automatically.

## Updates and distribution

Windows 11 24H2 and newer is the first-release platform. MIDI 2.0 capabilities
are available only where the supported Windows service exists; 24H2 receives
the MIDI 1.0 fallback.

Releases provide a signed installer and a self-contained portable package.
Both include version-matched Tauri, Rust-engine, and Python-sidecar artifacts.
An installed or portable build must not silently install missing drivers,
runtimes, models, or permissions during startup.

Signed update metadata may be checked and downloaded only outside performance
mode. Installation occurs only after the DJ exits Live or explicitly schedules
it. Mapping/profile updates follow the same signature and rollback rules.

## Initial hardware evidence matrix

The initial physical matrix is deliberately cross-brand:

- **Numark Mixtrack Platinum:** generic class-compliant MIDI and entry-level
  capacitive jog baseline;
- **Pioneer DJ DDJ-1000:** club-layout controls, published MIDI message list,
  and multichannel controller/interface behavior;
- **Rane ONE MKII:** integrated motorized-platter and audio-interface behavior.

These devices are treated as MIDI 1.0 unless their manufacturer explicitly
documents otherwise. A high-resolution display or jog claim is not evidence of
MIDI 2.0. Vendor-only HID displays and undocumented commands are outside a
certification claim until implemented from authorized documentation and tested.

## Delivery stages

Each stage is an end-to-end usable slice with focused automated tests, manual
hardware evidence, migration notes, and no claim beyond what was exercised.

1. **Architecture and desktop foundation:** Proposed ADR, monorepo layout,
   Tauri shell, supervised/authenticated Python sidecar, Rust engine lifecycle,
   packaging skeleton, action-registry contract, and CI lanes.
2. **Audio and shared library:** ASIO/WASAPI discovery and stable output,
   multichannel graph, master/cue/direct buses, basic inputs, catalog access,
   concurrent ownership, journal skeleton, and device-failure simulation.
3. **Actions and mappings:** complete keyboard editor, MIDI 1 fallback,
   Windows MIDI Services detection, MIDI 2/UMP path, multiple devices, learn,
   advanced editor, feedback, profile migration, Clock/MTC, and auto-detection.
4. **Live decks and scratch:** two decks, waveform/beat-grid editing, cue/loop,
   sync, ownership handoff, touch jog and motorized scratch model, routing UI,
   recording, and the three-device evidence matrix.
5. **Advanced performance:** full FX/racks, sixteen-slot sampler, offline stems,
   GPU acceleration, third-party library import, optional stream handoff, and
   accessible performance workflows.
6. **Release hardening:** recovery and fallback drills, underrun/latency runs,
   long-set soak tests, installer/portable upgrades, signing, security review,
   screen-reader samples, documentation, and explicit compatibility claims.

Later stages may refine earlier behavior, but a stage cannot bypass the
real-time, security, persistence, or accessibility boundaries to demo a
feature.

## Verification and acceptance

Before implementation begins, the project needs an authoritative GitHub issue
with observable acceptance criteria and
[Proposed ADR 14](../../adr/0014-separate-autodj-live-realtime-ownership.md) for
the new ownership boundaries. Detailed implementation planning follows
approval of this design.

Acceptance evidence must include:

- automated unit/property tests for DSP state, mapping transforms, action
  guards, profile migrations, route validation, ownership generations, and
  recovery journals;
- integration tests for Tauri/engine/sidecar lifecycle, authenticated loopback
  IPC, shared-catalog concurrency, automation takeover, recording/stream
  handoff, and device loss;
- virtual/fake audio and MIDI backends for deterministic CI, without citing
  them as physical-hardware evidence;
- Windows hardware runs for every claimed backend, sample rate, buffer size,
  channel layout, and certification controller;
- scratch response, motor feedback, MIDI timing/jitter, audio latency,
  underrun, drift-bridge, CPU/GPU load, and long-set soak measurements;
- accessibility automation plus recorded NVDA/browser or native-WebView speech
  samples for the named flows and exact versions tested;
- signed installer and portable clean-machine tests, offline startup, upgrade,
  rollback, and unclean-shutdown recovery;
- license/source/notices review for ASIO, Tauri/Rust dependencies, bundled
  Python packages, models, and imported controller profiles.

The existing architecture checker and affected AutoDJ tests remain mandatory.
New ADR contracts must distinguish automated, manual, and gap evidence. A green
CI run does not establish real-time latency, hardware compatibility, screen-
reader support, or club readiness.

Stage 1 must publish the exact reference computers, Windows builds, drivers,
firmware, measurement tools, and pass/fail budgets before Stage 2 is accepted.
The starting release gate is a four-hour continuous 48 kHz/128-sample set with
zero engine callback underruns on each reference audio path, no unbounded drift
or queue growth, and measured end-to-end scratch response within the agreed
5-10 ms target. Other rates and layouts receive their own recorded runs. MIDI
jitter, bridge drift, CPU headroom, thermal behavior, and recovery timing get
numeric budgets from the Stage 1 measurement spike; those accepted numbers are
then versioned requirements rather than being invented after implementation.

## Builder-owned implementation choices

The following choices may be made during the implementation plan or Proposed
ADR without changing product intent, provided the resulting design meets the
constraints above:

- exact Rust crate selection and IPC serialization format;
- repository folder names and internal module boundaries;
- fixed graph representation and resampler/DSP implementations;
- exact human-readable mapping syntax and schema identifiers;
- stem model selection after license, quality, CPU, GPU, and size evaluation;
- code-signing provider and installer technology;
- precise profile-update hosting and signing mechanism;
- color palette and visual styling within the agreed club layout and
  accessibility requirements.
- the precise reference computer models and Stage 1 numeric MIDI, drift,
  thermal, and recovery budgets after the required measurement spike.

Any choice that weakens offline operation, adds a cloud account, changes the
license, modifies third-party DJ databases, changes the two-deck product model,
or moves club-critical work back into the WebView or Python event loop requires
a new product decision.
