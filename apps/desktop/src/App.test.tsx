import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { App } from "./App.tsx";
import { usePreferences } from "./stores/preferences.ts";
import { emitCoreEvent, fakeDownload, fakeItem, fakePlugin, installFakeCore, type FakeCore } from "./test/fakeCore.ts";

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

  it("shows Cove achievements with progress on Home", async () => {
    renderAt("/");
    expect(await screen.findByText("Erste Stunde")).toBeDefined();
    expect(screen.getByRole("progressbar", { name: "Fortschritt 10 Stunden" })).toBeDefined();
  });

  it("signs in to Epic with the alternative code login", async () => {
    renderAt("/settings/accounts");
    const buttons = await screen.findAllByRole("button", { name: "Anmelden" });
    const epic = buttons[2];
    if (!epic) throw new Error("login button missing");
    fireEvent.click(epic);
    fireEvent.click(await screen.findByText("Alternativer Login"));
    fireEvent.change(screen.getByLabelText("Anmeldecode"), { target: { value: "abc123" } });
    const submit = screen.getAllByRole("button", { name: "Anmelden" }).find((b) => b.closest("details"));
    if (!submit) throw new Error("submit missing");
    fireEvent.click(submit);
    expect(await screen.findByText(/Verbunden als Tiny/)).toBeDefined();
    expect(core.calls.some((c) => c.command === "connector_login_complete")).toBe(true);
  });
});

describe("Language", () => {
  it("switches the whole UI to English and saves it in the core", async () => {
    renderAt("/settings/general");
    const select = await screen.findByRole("combobox", { name: "Sprache" });
    fireEvent.change(select, { target: { value: "en" } });
    expect(await screen.findByRole("combobox", { name: "Language" })).toBeDefined();
    expect(screen.getByRole("navigation", { name: "Sections" })).toBeDefined();
    expect(screen.getByRole("link", { name: "Accounts & stores" })).toBeDefined();
    expect(document.documentElement.lang).toBe("en");
    await waitFor(() => {
      expect(core.settings.general.language).toBe("en");
    });
  });

  it("starts in the language saved in the core", async () => {
    core.settings.general.language = "en";
    renderAt("/library");
    expect(await screen.findByRole("button", { name: /Add game/ })).toBeDefined();
    expect(screen.getByRole("navigation", { name: "Grouping" })).toBeDefined();
    expect(screen.getByRole("button", { name: /Favourites/ })).toBeDefined();
  });
});

describe("Catalogue", () => {
  it("shows catalogue games from the core", async () => {
    renderAt("/catalogue");
    const list = await screen.findByRole("list", { name: "Spiele" });
    expect(within(list).getAllByText("Hollow Lantern").length).toBeGreaterThan(0);
    expect(within(list).getAllByText("Paper Orbit").length).toBeGreaterThan(0);
    expect(screen.queryByText("Offline")).toBeNull();
  });

  it("works without own keys through the Cove service", async () => {
    expect(core.catalog.igdb).toBe(false);
    renderAt("/catalogue");
    expect(await screen.findByRole("list", { name: "Spiele" })).toBeDefined();
  });

  it("explains when no catalogue source is available", async () => {
    core.catalog.cove = false;
    renderAt("/catalogue");
    expect(await screen.findByRole("heading", { name: "Katalog nicht verfügbar" })).toBeDefined();
    expect(screen.getByRole("link", { name: "Zu den Einstellungen" })).toBeDefined();
  });

  it("marks cached data shown while offline", async () => {
    core.catalog.offline = true;
    renderAt("/catalogue");
    expect(await screen.findByText("Offline")).toBeDefined();
  });

  it("links the detail page to the owned library game", async () => {
    core.library[0] = fakeItem("tidewater", "Tidewater", { catalogId: "igdb-1", matchStatus: "matched" });
    renderAt("/catalogue/igdb-1");
    expect(await screen.findByRole("link", { name: /In deiner Bibliothek/ })).toBeDefined();
    renderAt("/catalogue/igdb-2");
    expect(await screen.findByRole("button", { name: /Zur Bibliothek hinzufügen/ })).toBeDefined();
  });
});

describe("Catalogue matching", () => {
  it("lets the user confirm one of several suggestions", async () => {
    core.library[0] = fakeItem("tidewater", "Tidewater", { matchStatus: "pending" });
    core.candidates.tidewater = [
      { game: { ...fakeItem("igdb-1", "Tidewater").game, id: "igdb-1" }, score: 0.97 },
      { game: { ...fakeItem("igdb-9", "Tidewater Remastered").game, id: "igdb-9" }, score: 0.9 },
    ];
    renderAt("/library/tidewater");
    fireEvent.click(await screen.findByRole("button", { name: "Zuordnung prüfen" }));
    const dialog = await screen.findByRole("dialog", { name: "Katalog-Zuordnung" });
    expect(await within(dialog).findByText("97 %")).toBeDefined();
    const [first] = within(dialog).getAllByRole("button", { name: "Zuordnen" });
    if (!first) throw new Error("no candidate");
    fireEvent.click(first);
    await waitFor(() => {
      expect(core.library[0]?.entry.catalogId).toBe("igdb-1");
    });
    expect(await screen.findByText(/zugeordnet, Metadaten und Cover übernommen/)).toBeDefined();
  });
});

function firstSaveButton(): HTMLElement {
  const [button] = screen.getAllByRole("button", { name: "Speichern & prüfen" });
  if (!button) throw new Error("save button missing");
  return button;
}

describe("Catalogue settings", () => {
  it("shows the Cove service as ready and the fallback order", async () => {
    renderAt("/settings/catalog");
    expect(await screen.findByText("Bereit, keine Einrichtung nötig")).toBeDefined();
    expect(screen.getByText(/Reihenfolge: Cove-Katalogdienst, zuletzt gespeicherte Daten/)).toBeDefined();
    fireEvent.click(screen.getByRole("switch", { name: "Cove-Katalogdienst verwenden" }));
    await waitFor(() => {
      expect(core.settings.catalog.useCoveService).toBe(false);
    });
    expect(await screen.findByText("Katalog nicht verfügbar")).toBeDefined();
  });

  it("verifies own credentials and shows provider errors inline", async () => {
    renderAt("/settings/catalog");
    fireEvent.change(await screen.findByLabelText("Client-ID"), { target: { value: "id" } });
    fireEvent.change(screen.getByLabelText("Client-Secret"), { target: { value: "wrong" } });
    fireEvent.click(firstSaveButton());
    expect(await screen.findByText(/Client-ID oder Secret falsch/)).toBeDefined();

    fireEvent.change(screen.getByLabelText("Client-Secret"), { target: { value: "right" } });
    fireEvent.click(firstSaveButton());
    expect(await screen.findByText("IGDB verbunden")).toBeDefined();
    expect(core.catalog.igdb).toBe(true);
    expect(await screen.findByText(/Cove-Katalogdienst, dann eigene IGDB-Keys/)).toBeDefined();
  });

  it("credits IGDB in About", async () => {
    renderAt("/settings/about");
    const link = await screen.findByRole("link", { name: /IGDB\.com/ });
    expect(link.getAttribute("href")).toBe("https://www.igdb.com");
    expect(screen.getByText(/Game data provided by/)).toBeDefined();
  });
});

describe("About and updates", () => {
  it("shows which services run and what is off without them", async () => {
    core.recommender = false;
    renderAt("/settings/about");
    expect(await screen.findByText("Nicht verfügbar")).toBeDefined();
    expect(screen.getByText(/einfache Vorschläge statt persönlicher Empfehlungen/)).toBeDefined();
    expect(screen.getByText("Startet bei Bedarf")).toBeDefined();
  });

  it("says when Cove is up to date", async () => {
    renderAt("/settings/about");
    fireEvent.click(await screen.findByRole("button", { name: "Nach Updates suchen" }));
    expect(await screen.findByText("Cove ist auf dem neuesten Stand.")).toBeDefined();
  });

  it("installs an available update", async () => {
    core.update = { version: "0.2.0", notes: "Ruhiger und schneller.", date: null };
    renderAt("/settings/about");
    fireEvent.click(await screen.findByRole("button", { name: "Nach Updates suchen" }));
    expect(await screen.findByText(/ist verfügbar/)).toBeDefined();
    expect(screen.getByText("Ruhiger und schneller.")).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: "Installieren und neu starten" }));
    await waitFor(() => {
      expect(core.updateInstalled).toBe(true);
    });
  });
});

describe("Errors", () => {
  it("never shows technical exception text", async () => {
    const { errorMessage } = await import("./stores/toasts.ts");
    const quiet = console.error;
    console.error = () => undefined;
    try {
      expect(errorMessage(new TypeError("Cannot read properties of undefined (reading 'x')"))).toBe(
        "Da ist etwas schiefgelaufen. Versuch es noch einmal.",
      );
    } finally {
      console.error = quiet;
    }
  });
});

describe("Recommendations", () => {
  it("shows recommender reasons on Home", async () => {
    renderAt("/");
    const recommended = await screen.findByRole("region", { name: "Für dich empfohlen" });
    expect((await within(recommended).findAllByText("Weil du 40 Std. Tidewater gespielt hast")).length).toBeGreaterThan(0);
    const discover = screen.getByRole("region", { name: "Neu entdecken" });
    expect((await within(discover).findAllByText("Neu für dich: Horror")).length).toBeGreaterThan(0);
  });

  it("falls back to a simple ranking when the recommender is unavailable", async () => {
    core.recommender = false;
    renderAt("/");
    const recommended = await screen.findByRole("region", { name: "Für dich empfohlen" });
    expect((await within(recommended).findAllByText(/Passt zu deinen Genres|Hoch bewertet/)).length).toBeGreaterThan(0);
    const discover = screen.getByRole("region", { name: "Neu entdecken" });
    expect((await within(discover).findAllByText("Neu im Katalog")).length).toBeGreaterThan(0);
  });

  it("shows the reason under catalogue cards when sorted by recommendation", async () => {
    renderAt("/catalogue");
    const list = await screen.findByRole("list", { name: "Spiele" });
    expect(within(list).getByText("Ähnlich wie Hollow Lantern")).toBeDefined();
  });
});

describe("Downloads", () => {
  it("shows live progress from the core", async () => {
    core.downloads = [fakeDownload("d1", "Lantern", { status: "downloading", totalBytes: 1000, downloadedBytes: 0 })];
    renderAt("/downloads");
    expect(await screen.findByRole("article", { name: "Download Lantern" })).toBeDefined();
    await act(() => emitCoreEvent("download-progress", { id: "d1", status: "downloading", downloadedBytes: 500, totalBytes: 1000, bytesPerSecond: 250 }));
    // Page and sidebar indicator both follow the event.
    expect(await screen.findAllByText("50 %")).toHaveLength(2);
    expect(screen.getByRole("progressbar", { name: "Fortschritt Lantern" }).getAttribute("aria-valuenow")).toBe("500");
  });

  it("adds a direct download and validates the link", async () => {
    renderAt("/downloads");
    fireEvent.click(await screen.findByRole("button", { name: "Download per Link" }));
    const dialog = await screen.findByRole("dialog", { name: "Download per Link" });
    fireEvent.change(within(dialog).getByLabelText("Link"), { target: { value: "ftp://nope" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Herunterladen" }));
    expect(await within(dialog).findByText("Das ist kein gültiger Link.")).toBeDefined();
    fireEvent.change(within(dialog).getByLabelText("Link"), { target: { value: "https://example.com/lantern.zip" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Herunterladen" }));
    await waitFor(() => {
      expect(core.downloads).toHaveLength(1);
    });
    expect(await screen.findByText("lantern.zip")).toBeDefined();
  });

  it("hands store installs to the client and shows that", async () => {
    core.library = [fakeItem("steam-620", "Portal 2", { source: "steam", storeGameId: "620", installed: false, executablePath: null })];
    core.downloads = [];
    renderAt("/library/steam-620");
    fireEvent.click(await screen.findByRole("button", { name: "Installieren" }));
    await waitFor(() => {
      expect(core.downloads[0]?.download.status).toBe("handedOff");
    });
    renderAt("/downloads");
    expect((await screen.findAllByText(/Installation läuft in Steam/)).length).toBeGreaterThan(0);
  });

  it("downloads an owned itch.io game", async () => {
    core.itchKey = "goodkey";
    renderAt("/downloads");
    fireEvent.click(await screen.findByRole("button", { name: "Von itch.io" }));
    const dialog = await screen.findByRole("dialog", { name: "Von itch.io herunterladen" });
    fireEvent.click(await within(dialog).findByRole("button", { name: "Zeta" }));
    fireEvent.click(await within(dialog).findByRole("button", { name: "Herunterladen" }));
    await waitFor(() => {
      expect(core.downloads[0]?.download.origin).toBe("itch");
    });
  });

  it("asks to connect itch.io first", async () => {
    renderAt("/downloads");
    fireEvent.click(await screen.findByRole("button", { name: "Von itch.io" }));
    expect(await screen.findByText("itch.io ist nicht verbunden")).toBeDefined();
  });
});

describe("itch.io account", () => {
  it("connects with an API key", async () => {
    renderAt("/settings/accounts");
    const input = await screen.findByLabelText("itch.io-API-Schlüssel");
    fireEvent.change(input, { target: { value: "wrong" } });
    fireEvent.click(screen.getByRole("button", { name: "Verbinden" }));
    expect(await screen.findByText("Der API-Schlüssel ist ungültig oder abgelaufen.")).toBeDefined();
    fireEvent.change(screen.getByLabelText("itch.io-API-Schlüssel"), { target: { value: "goodkey" } });
    fireEvent.click(screen.getByRole("button", { name: "Verbinden" }));
    expect(await screen.findByText("Verbunden als Kiefer")).toBeDefined();
  });
});

describe("Plugins settings", () => {
  it("shows requested permissions before a plugin may run", async () => {
    core.plugins = [fakePlugin("random-pick", { name: "Zufallsauswahl", permissions: ["library.read", "ui.notify"] })];
    renderAt("/settings/plugins");
    expect(await screen.findByText(/Wartet auf deine Freigabe/)).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: "Prüfen" }));
    const dialog = await screen.findByRole("dialog", { name: "„Zufallsauswahl“ hinzufügen?" });
    expect(within(dialog).getByText("Bibliothek lesen")).toBeDefined();
    expect(within(dialog).getByText("Hinweise anzeigen")).toBeDefined();
    fireEvent.click(within(dialog).getByRole("button", { name: "Erlauben und aktivieren" }));
    await waitFor(() => {
      expect(core.plugins[0]?.status).toBe("active");
    });
    expect(core.plugins[0]?.granted).toEqual(["library.read", "ui.notify"]);
    expect(await screen.findByRole("switch", { name: "Zufallsauswahl aktivieren" })).toBeDefined();
  });

  it("installs from a folder after confirming permissions", async () => {
    renderAt("/settings/plugins");
    fireEvent.click(await screen.findByRole("button", { name: "Plugin installieren" }));
    const dialog = await screen.findByRole("dialog", { name: "Plugin installieren" });
    fireEvent.change(within(dialog).getByLabelText("Ordner"), { target: { value: "/tmp/nope" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Weiter" }));
    expect(await within(dialog).findByText(/mit plugin.json/)).toBeDefined();
    fireEvent.change(within(dialog).getByLabelText("Ordner"), { target: { value: "/tmp/csv-plugin" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Weiter" }));
    const review = await screen.findByRole("dialog", { name: "„CSV-Import“ hinzufügen?" });
    expect(within(review).getByText("Spiele hinzufügen")).toBeDefined();
    fireEvent.click(within(review).getByRole("button", { name: "Installieren" }));
    await waitFor(() => {
      expect(core.plugins.map((p) => p.id)).toEqual(["csv-import"]);
    });
  });

  it("shows broken plugins with the reason", async () => {
    core.plugins = [fakePlugin("broken", { status: "invalid", error: "plugin.json ist ungültig (expected value)." })];
    renderAt("/settings/plugins");
    expect(await screen.findByText("plugin.json ist ungültig (expected value).")).toBeDefined();
    expect(screen.queryByRole("switch")).toBeNull();
  });
});
