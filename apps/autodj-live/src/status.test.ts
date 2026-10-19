// @vitest-environment happy-dom

import { beforeEach, describe, expect, test, vi } from "vitest";

import {
  announceStatus,
  mountStatusSurface,
  type FoundationApi,
  type FoundationStatus,
} from "./status";

const READY: FoundationStatus = {
  state: "ready",
  engine: "ready",
  sidecar: "ready",
  uptimeSeconds: 12,
};

function api(status: FoundationStatus = READY): FoundationApi {
  return {
    getStatus: vi.fn().mockResolvedValue(status),
    retry: vi.fn().mockResolvedValue(status),
    shutdown: vi.fn().mockResolvedValue(undefined),
  };
}

describe("foundation status surface", () => {
  beforeEach(() => {
    document.body.replaceChildren();
  });

  test("shows starting state before the first status response", () => {
    mountStatusSurface(document.body, api());

    expect(document.querySelector("[data-component='engine'] dd")?.textContent).toBe("Starting");
    expect(document.querySelector("[data-component='sidecar'] dd")?.textContent).toBe("Starting");
  });

  test("shows engine and sidecar state as visible text", async () => {
    const surface = mountStatusSurface(document.body, api());
    await surface.refresh();

    expect(document.querySelector("h1")?.textContent).toBe("AutoDJ Live");
    expect(document.querySelector("[data-component='engine'] dd")?.textContent).toBe("Ready");
    expect(document.querySelector("[data-component='sidecar'] dd")?.textContent).toBe("Ready");
  });

  test("announces a changed status once and leaves identical polls silent", () => {
    const region = document.createElement("p");
    region.setAttribute("role", "status");

    expect(announceStatus(region, "AutoDJ Live is ready.")).toBe(true);
    const textNode = region.firstChild;
    expect(announceStatus(region, "AutoDJ Live is ready.")).toBe(false);
    expect(region.firstChild).toBe(textNode);
  });

  test("names the failed component and offers its retry action", async () => {
    const degraded: FoundationStatus = {
      state: "degraded",
      engine: "ready",
      sidecar: "failed",
      uptimeSeconds: 4,
    };
    const surface = mountStatusSurface(document.body, api(degraded));
    await surface.refresh();

    expect(document.body.textContent).toContain("Sidecar needs attention");
    expect(document.querySelector("button")?.textContent).toBe("Retry sidecar");
  });

  test("retry retains keyboard focus", async () => {
    const degraded: FoundationStatus = {
      state: "degraded",
      engine: "ready",
      sidecar: "failed",
      uptimeSeconds: 4,
    };
    const client = api(degraded);
    const surface = mountStatusSurface(document.body, client);
    await surface.refresh();
    const button = document.querySelector("button") as HTMLButtonElement;
    button.focus();
    button.click();
    await vi.waitFor(() => expect(client.retry).toHaveBeenCalledWith("sidecar"));

    expect(document.activeElement).toBe(button);
  });

  test("keeps ticking uptime outside the live region", async () => {
    const surface = mountStatusSurface(document.body, api());
    await surface.refresh();
    surface.setUptime(13);

    const uptime = document.querySelector("[data-uptime]") as HTMLElement;
    expect(uptime.textContent).toBe("13 seconds");
    expect(uptime.closest("[role='status']")).toBeNull();
    expect(uptime.getAttribute("aria-live")).toBe("off");
    expect(document.querySelectorAll("[role='status']")).toHaveLength(1);
  });

  test("never renders credentials or transport details", async () => {
    const unsafe = {
      ...READY,
      token: "top-secret",
      port: 43123,
      pid: 999,
    } as FoundationStatus;
    const surface = mountStatusSurface(document.body, api(unsafe));
    await surface.refresh();

    expect(document.body.textContent).not.toMatch(/top-secret|43123|999/);
  });
});
