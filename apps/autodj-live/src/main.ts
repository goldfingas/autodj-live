import { invoke } from "@tauri-apps/api/core";

import { mountStatusSurface, type Component, type FoundationStatus } from "./status";
import { mountAudioSetup, type AudioDiscovery, type WasapiCapabilities } from "./setup";

const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("AutoDJ Live root is unavailable");

const surface = mountStatusSurface(root, {
  getStatus: () => invoke<FoundationStatus>("get_foundation_status"),
  retry: (component: Component) => invoke<FoundationStatus>("retry_component", { component }),
  shutdown: () => invoke<void>("shutdown_live"),
});
mountAudioSetup(root, {
  discover: () => invoke<AudioDiscovery>("get_audio_discovery"),
  probe: (endpointId) => invoke<WasapiCapabilities>("probe_wasapi_endpoint", { endpointId }),
});

void surface.refresh();
window.setInterval(() => void surface.refresh(), 2_000);
