import { invoke } from "@tauri-apps/api/core";

import { mountStatusSurface, type Component, type FoundationStatus } from "./status";

const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("AutoDJ Live root is unavailable");

const surface = mountStatusSurface(root, {
  getStatus: () => invoke<FoundationStatus>("get_foundation_status"),
  retry: (component: Component) => invoke<FoundationStatus>("retry_component", { component }),
  shutdown: () => invoke<void>("shutdown_live"),
});

void surface.refresh();
window.setInterval(() => void surface.refresh(), 2_000);
