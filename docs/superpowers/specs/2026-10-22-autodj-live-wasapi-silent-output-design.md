# AutoDJ Live WASAPI Silent Output Design

## Intent

Advance the Windows audio path from transient shared-buffer initialization to
an explicit, bounded render exercise. A DJ can select an active output endpoint
in Setup and ask AutoDJ Live to run one second of event-driven shared-mode
silence. This proves that the engine can start, service, stop, and release a
render stream without making an audible test tone or claiming production
playback readiness.

## Goals

- Exercise only an active WASAPI render endpoint selected by the user.
- Configure the same non-offloaded `AudioCategory_Media` shared stream used by
  the initialization checkpoint.
- Request 128 frames when the configured shared-period range accepts it and
  otherwise use the reported default.
- Initialize with `AUDCLNT_STREAMFLAGS_EVENTCALLBACK`, register an event before
  `Start`, prefill the buffer with silence, then service event notifications for
  one second.
- On each notification, subtract `GetCurrentPadding` from the allocated buffer
  size and submit the available frames with `AUDCLNT_BUFFERFLAGS_SILENT`.
- Stop and release the stream on success and on every post-start failure.
- Return bounded evidence: endpoint ID, requested period, allocated frames,
  event notifications, submitted silent frames, and elapsed milliseconds.
- Preserve authenticated ownership, one-audio-operation-at-a-time behavior,
  older-engine compatibility, refresh-generation handling, and generic UI
  errors.

## Non-goals and anti-goals

- No decoded music, audible tone, mixing, route publication, input capture,
  exclusive mode, ASIO, resampling, or secondary-device drift handling.
- No MMCSS/thread-priority claim, callback-time allocation guarantee,
  underrun count, latency measurement, long-run stability claim, or recovery
  after device loss.
- Do not label a successful one-second silent exercise as playback-ready.
- Do not expose the action for capture endpoints or run it automatically.
- Do not keep a stream alive after the action finishes.

## Approaches considered

1. **Bounded event-driven silent render (selected).** Reuse the direct WASAPI
   client and existing worker boundary, add one Windows event handle, and submit
   silent packets for one second. This is the smallest check of the event path
   needed by later playback.
2. **Timer-driven silent render.** This would exercise `Start` and buffer writes
   but would not validate the event-callback mode selected for the Windows
   engine, so it was rejected.
3. **Persistent production output host.** This would require route publication,
   shutdown arbitration, real-time scheduling, device-loss recovery, and soak
   evidence in one change. It is the later destination, not a safe checkpoint.

The user repeatedly directed the agent to continue without design questions,
so approach 1 is the agent's scoped choice rather than a user-selected option.

## Design

### Native render exercise

Add `run_wasapi_shared_output_test(endpoint_id)` beside the existing explicit
probe and initialization functions. The worker owns its COM apartment, event
handle, client, and render service on one thread. It rejects empty IDs,
inactive endpoints, and endpoints whose `IMMEndpoint::GetDataFlow` is not
`eRender` before initialization.

The operation obtains and validates the exact shared mix format, sets Media
client properties, re-queries and validates shared periods on that configured
client, initializes with the event-callback flag, and reads the real buffer
capacity. It registers an auto-reset event before starting. It fills the
initial available buffer as silent, starts, waits for events, and on each event
submits all currently available frames as silent. A wait that does not signal
within 250 ms, an abandoned/failed wait, invalid padding, a zero-event run, or
any WASAPI error fails the operation. A started-client guard calls `Stop` on
every return path.

The one-second deadline is measured with `Instant`; elapsed time is evidence,
not an audio-clock or latency measurement. The result validator rejects empty
identity, zero periods/buffer/events/frames, and elapsed values below the
requested one-second exercise.

### Process boundary and compatibility

Add a typed result plus `RunWasapiSharedOutputTest` command/event. Advertise a
new default-false capability in `ServerHello`. The supervisor rejects the
operation locally when attached to an older compatible engine, so it never
writes an unknown enum variant and does not consume the healthy connection.

The engine authorizes the command for the active owner and runs it through the
existing single-flight audio worker. The error returned outside the engine is
generic and contains no endpoint or driver detail.

### Setup interface

For each active output endpoint, show **Run silent output test** next to the
existing diagnostic actions. The pending label states that the output device is
being opened and silence is being submitted. Success reports the period,
allocated frames, event count, silent frames, and elapsed time, followed by a
plain warning that this is not music playback or latency evidence.

Refreshing discovery immediately invalidates a pending or completed result.
The button is keyboard-operable, restores focus after completion, and remains
usable after a failed refresh. Capture endpoints never show this action.

## Testing and evidence

- Unit-test result validation and period selection without hardware.
- Round-trip the new protocol types; decode an older hello with capability
  false; prove an unsupported request is rejected without consuming the socket.
- Test owner authorization and engine mapping with existing seams.
- Test output-only UI visibility, pending/success/error text, focus restoration,
  duplicate endpoint identity, refresh invalidation, and single-click behavior.
- Keep native success opt-in through `AUTODJ_LIVE_WASAPI_OUTPUT_ENDPOINT_ID` so
  the default suite does not assume hardware availability or shared access.
- Run the complete Rust, UI, policy, architecture, and production-build gates.
- When a selected local render endpoint is available, record only its format,
  period, allocated frames, notification count, silent-frame count, elapsed
  time, and clean stop. One run is not a soak test.

## Primary API references

- [InitializeSharedAudioStream](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient3-initializesharedaudiostream)
- [SetEventHandle](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-seteventhandle)
- [IAudioRenderClient::GetBuffer](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudiorenderclient-getbuffer)
- [IAudioRenderClient::ReleaseBuffer](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudiorenderclient-releasebuffer)
- [GetCurrentPadding](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-getcurrentpadding)
