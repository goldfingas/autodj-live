// @vitest-environment happy-dom

import { beforeEach, describe, expect, test, vi } from "vitest";

import { mountAudioSetup, type AudioDiscovery } from "./setup";

const DISCOVERY: AudioDiscovery = {
  endpoints: [
    { id: "output-1", name: "Speakers", direction: "output", state: "active" },
    { id: "output-2", name: "Speakers", direction: "output", state: "disabled" },
    { id: "input-1", name: "Mic 1/2", direction: "input", state: "active" },
  ],
  asioDrivers: [
    { registration: "Rane PERFORMER ASIO", clsid: "{6e9aac96-aeda-4f9a-b222-13ea83facee8}" },
  ],
  issues: ["One stale endpoint name was unavailable"],
};

describe("audio setup discovery", () => {
  beforeEach(() => document.body.replaceChildren());

  test("waits for the DJ to request discovery", () => {
    const discover = vi.fn().mockResolvedValue(DISCOVERY);
    mountAudioSetup(document.body, { discover });

    expect(document.querySelector("h2")?.textContent).toBe("Audio setup");
    expect(document.querySelector("button")?.textContent).toBe("Refresh audio devices");
    expect(discover).not.toHaveBeenCalled();
  });

  test("renders distinct endpoint identities and unprobed ASIO registrations", async () => {
    const discover = vi.fn().mockResolvedValue(DISCOVERY);
    mountAudioSetup(document.body, { discover });
    (document.querySelector("button") as HTMLButtonElement).click();

    await vi.waitFor(() => expect(document.querySelectorAll("[data-endpoint-id]")).toHaveLength(3));
    const speakers = [...document.querySelectorAll("[data-endpoint-id]")]
      .filter((item) => item.textContent?.includes("Speakers"));
    expect(speakers).toHaveLength(2);
    expect(speakers.map((item) => (item as HTMLElement).dataset.endpointId)).toEqual(["output-1", "output-2"]);
    expect(document.body.textContent).toContain("Output · Active");
    expect(document.body.textContent).toContain("Input · Active");
    expect(document.body.textContent).toContain("Rane PERFORMER ASIO · Registered, capabilities unprobed");
    expect(document.body.textContent).toContain("One stale endpoint name was unavailable");
    expect(document.body.textContent).not.toContain("6e9aac96");
  });

  test("replaces results and announces one summary per completed refresh", async () => {
    const discover = vi.fn()
      .mockResolvedValueOnce(DISCOVERY)
      .mockResolvedValueOnce({ endpoints: [], asioDrivers: [], issues: [] });
    mountAudioSetup(document.body, { discover });
    const button = document.querySelector("button") as HTMLButtonElement;
    button.focus();
    button.click();
    await vi.waitFor(() => expect(document.querySelectorAll("[data-endpoint-id]")).toHaveLength(3));
    button.click();
    await vi.waitFor(() => expect(document.querySelectorAll("[data-endpoint-id]")).toHaveLength(0));

    expect(document.querySelectorAll("[data-audio-announcement]")).toHaveLength(1);
    expect(document.querySelector("[data-audio-announcement]")?.textContent)
      .toBe("Found 0 audio endpoints and 0 registered ASIO drivers.");
    expect(document.activeElement).toBe(button);
  });

  test("restores the refresh action after a discovery failure", async () => {
    mountAudioSetup(document.body, { discover: vi.fn().mockRejectedValue(new Error("private detail")) });
    const button = document.querySelector("button") as HTMLButtonElement;
    button.click();
    expect(button.disabled).toBe(true);
    await vi.waitFor(() => expect(button.disabled).toBe(false));
    expect(document.body.textContent).toContain("Audio devices could not be refreshed.");
    expect(document.body.textContent).not.toContain("private detail");
  });

  test("does not steal focus when the DJ moves on during refresh", async () => {
    let resolve: { value?: (discovery: AudioDiscovery) => void } = {};
    const pending = new Promise<AudioDiscovery>((done) => { resolve.value = done; });
    mountAudioSetup(document.body, { discover: vi.fn().mockReturnValue(pending) });
    const refresh = document.querySelector("button") as HTMLButtonElement;
    const other = document.createElement("button");
    other.textContent = "Other action";
    document.body.append(other);
    refresh.focus();
    refresh.click();
    other.focus();

    resolve.value?.(DISCOVERY);
    await vi.waitFor(() => expect(refresh.disabled).toBe(false));

    expect(document.activeElement).toBe(other);
  });
});
