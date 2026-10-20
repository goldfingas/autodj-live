# Audio discovery checkpoint

## Reference hardware

On 2026-10-20 the operator selected **Rane PERFORMER** as the primary
reference for the first hardware-audio tests. This selection supplements
the original product design's reference-controller matrix; it does not
claim compatibility or replace the wider release matrix.

Read-only local inventory found a `Rane PERFORMER` Windows sound device
with status `OK`, and `Rane PERFORMER ASIO` registered in both Windows
registry views. AMD and Realtek sound devices were also reported.
Driver registration alone does not establish connected hardware, successful
driver initialization, channel layout, sample-rate support, or latency.
No stream was opened, driver loaded explicitly, or system audio setting changed
for this inventory.

## First discovery boundary

Discovery must distinguish three different levels of evidence:

1. Registered ASIO driver: identity/registration only, not capabilities.
2. Windows audio endpoint: input/output direction, endpoint ID, display name,
   and availability state, not a guaranteed physical-device channel map.
3. Explicitly probed driver/endpoint: negotiated mode, format, sample rate,
   channels, and buffer constraints, with errors retained per device.

Do not populate routing capabilities from guessed stereo defaults. Never
substitute a Windows shared-mode format for ASIO or exclusive-mode capability.
Multiple endpoints can refer to the same physical device; display names are
not unique identifiers. Do not select a fallback merely because the preferred
device is absent.

For the first implementation, list Windows endpoints and 64-bit registered
ASIO drivers without starting playback. Report ASIO entries as unprobed.
Capability probing and driver loading are a separate, explicit action.
The x64 application must not offer a 32-bit-only driver as loadable.

## Primary-source research

- Microsoft's [EnumAudioEndpoints documentation](https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nf-mmdeviceapi-immdeviceenumerator-enumaudioendpoints)
  defines capture/render directions and active/disabled/unplugged/not-present
  states, allowing enumeration without creating a stream.
- Microsoft's [GetMixFormat documentation](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-getmixformat)
  distinguishes the shared audio engine's mix format from exclusive hardware
  support and points to `IsFormatSupported` for mode-specific checks.
- [RustAudio CPAL](https://github.com/RustAudio/cpal) offers WASAPI and optional
  ASIO backends. Its presence alone does not establish that its pinned backend
  implementation meets this product's exclusive/shared-mode and channel
  requirements; inspect those paths before selecting it.
- [Steinberg's developer site](https://www.steinberg.net/developers/) lists an
  open-source ASIO option. Before integration, pin the actual SDK distribution
  and audit its license, notices, corresponding source, and build dependencies.
  No SDK or vendor driver has been downloaded or added at this checkpoint.

Backend selection remains open between direct Windows/SDK integration and
an existing Rust host library. Prefer reuse only where the actual APIs satisfy
the required modes and channel control. No new dependency is selected here.
