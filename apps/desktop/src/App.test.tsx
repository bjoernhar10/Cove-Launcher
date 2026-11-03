import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { App } from "./App.tsx";
import { usePreferences } from "./stores/preferences.ts";
import { emitCoreEvent, fakeItem, installFakeCore, type FakeCore } from "./test/fakeCore.ts";

let core: FakeCore;

beforeEach(() => {
  core = installFakeCore([
    fakeItem("tidewater", "Tidewater"),
    fakeItem("orbit", "Orbit", { installed: false, executablePath: null, installPath: null, lastPlayedAt: null, playtimeMinutes: 0 }),
  ]);
  usePreferences.setState({ theme: "system", accent: "blue", sidebarCollapsed: false, libraryView: "grid", libraryGroup: "all" });
});

function renderAt(path: string) {
  window.location.hash = `#${path}`;
  return render(<App />);
}

describe("App shell", () => {
  it("renders navigation and the profile from the core", async () => {
    renderAt("/");
    const nav = screen.getByRole("navigation", { name: "Bereiche" });
    for (const label of ["Home", "Catalogue", "My Library", "Downloads"]) {
      expect(within(nav).getByRole("link", { name: new RegExp(label) })).toBeDefined();
    }
    expect(await screen.findByText("Testperson")).toBeDefined();
    expect(await screen.findByRole("heading", { name: "Tidewater" })).toBeDefined();
  });

  it("opens the command palette with Ctrl+K and finds own games", async () => {
    renderAt("/");
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const input = await screen.findByRole("combobox", { name: "Suche" });
    fireEvent.change(input, { target: { value: "orb" } });
    expect(await screen.findByRole("option", { name: /Orbit.*In deiner Bibliothek/ })).toBeDefined();
  });

  it("collapses the sidebar to icons only", () => {
    renderAt("/");
    fireEvent.click(screen.getByRole("button", { name: /Seitenleiste einklappen/ }));
    expect(usePreferences.getState().sidebarCollapsed).toBe(true);
  });
});

describe("Library", () => {
  it("lists games from the core and switches to the list view", async () => {
    renderAt("/library");
    const list = await screen.findByRole("list", { name: "Spiele" });
    expect(within(list).getAllByText("Tidewater").length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("radio", { name: "Liste" }));
    expect(await screen.findByRole("table")).toBeDefined();
  });

  it("offers setup instead of play for manual games without executable", async () => {
    renderAt("/library");
    expect(await screen.findByRole("link", { name: /Orbit: Einrichten/ })).toBeDefined();
  });

  it("shows the running state from game-state-changed events", async () => {
    renderAt("/library/tidewater");
    expect(await screen.findByRole("button", { name: "Spielen" })).toBeDefined();
    await act(() => emitCoreEvent("game-state-changed", { gameId: "tidewater", state: "running", startedAt: "2026-11-03T10:00:00Z", sessionSeconds: null }));
    expect(await screen.findByRole("button", { name: /Läuft/ })).toBeDefined();
    await act(() => emitCoreEvent("game-state-changed", { gameId: "tidewater", state: "stopped", startedAt: "2026-11-03T10:00:00Z", sessionSeconds: 600 }));
    expect(await screen.findByText("Sitzung gespeichert: 10 Min.")).toBeDefined();
  });

  it("shows core validation errors in the add-game dialog", async () => {
    renderAt("/library");
    const [addButton] = await screen.findAllByRole("button", { name: "Spiel hinzufügen" });
    if (!addButton) throw new Error("add button missing");
    fireEvent.click(addButton);
    const dialog = await screen.findByRole("dialog", { name: "Spiel hinzufügen" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Hinzufügen" }));
    expect(await within(dialog).findByText("Bitte gib einen Namen ein.")).toBeDefined();

    fireEvent.change(within(dialog).getByLabelText("Name"), { target: { value: "Ghost" } });
    fireEvent.change(within(dialog).getByLabelText("Pfad zur .exe / Binary"), { target: { value: "/missing/ghost" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Hinzufügen" }));
    expect(await within(dialog).findByText("Datei nicht gefunden: /missing/ghost")).toBeDefined();
  });

  it("scans a folder and adds the confirmed suggestions", async () => {
    renderAt("/library");
    const [addButton] = await screen.findAllByRole("button", { name: "Spiel hinzufügen" });
    if (!addButton) throw new Error("add button missing");
    fireEvent.click(addButton);
    const dialog = await screen.findByRole("dialog", { name: "Spiel hinzufügen" });
    fireEvent.click(within(dialog).getByRole("radio", { name: "Ordner scannen" }));
    fireEvent.change(within(dialog).getByLabelText("Ordner"), { target: { value: "/games" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Scannen" }));

    const known = await within(dialog).findByRole("checkbox", { name: /Tidewater/ });
    expect((known as HTMLInputElement).disabled).toBe(true);
    fireEvent.click(within(dialog).getByRole("button", { name: "1 Spiel hinzufügen" }));
    await waitFor(() => {
      expect(core.library.some((i) => i.game.title === "Hollow Lantern")).toBe(true);
    });
  });
});

describe("Settings", () => {
  it("persists the theme in the core", async () => {
    renderAt("/settings/appearance");
    await waitFor(() => {
      expect(core.calls.some((c) => c.command === "settings_get")).toBe(true);
    });
    fireEvent.click(await screen.findByRole("radio", { name: /Dunkel/ }));
    expect(document.documentElement.dataset.theme).toBe("dark");
    await waitFor(() => {
      expect(core.settings.appearance.theme).toBe("dark");
    });
  });

  it("explains that store connections are not available yet", async () => {
    renderAt("/settings/accounts");
    const [connect] = await screen.findAllByRole("button", { name: "Verbinden" });
    if (!connect) throw new Error("connect button missing");
    fireEvent.click(connect);
    expect(await screen.findByText(/Connector-Service\./)).toBeDefined();
  });
});
