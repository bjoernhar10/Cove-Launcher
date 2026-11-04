import type { HostToPlugin, PluginInfo, PluginToHost } from "@cove/shared";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { fakeItem, fakePlugin, installFakeCore, type FakeCore } from "../test/fakeCore.ts";
import { PluginHost, type PluginPort, type PluginUi, type SidebarEntry } from "./host.ts";
import { runPlugin, type RuntimePort } from "./runtime.ts";

type Modules = Record<string, () => Promise<unknown>>;

/** Loads a plugin from the repository's /plugins folder (plain JS, no types). */
function examplePlugin(id: string): Promise<unknown> {
  return import(/* @vite-ignore */ new URL(`../../../../plugins/${id}/index.js`, import.meta.url).href);
}

/** Real plugin files from /plugins, plus small inline test plugins. */
const MODULES: Modules = {
  "random-pick": () => examplePlugin("random-pick"),
  "playtime-goals": () => examplePlugin("playtime-goals"),
  "csv-import": () => examplePlugin("csv-import"),
  sneaky: () =>
    Promise.resolve({
      activate: async (cove: { library: { list(): Promise<unknown> }; ui: { notify(m: string): Promise<void> } }) => {
        try {
          await cove.library.list();
          await cove.ui.notify("read the library");
        } catch (error: unknown) {
          await cove.ui.notify(`denied: ${(error as { kind: string }).kind}`);
        }
      },
    }),
  broken: () => Promise.resolve({ nothing: true }),
};

/** In-process stand-in for a worker: messages are structured-cloned like postMessage. */
function spawn(plugin: PluginInfo): PluginPort {
  const runtime: RuntimePort = { onmessage: null, postMessage: () => undefined };
  const port: PluginPort = {
    onmessage: null,
    onerror: null,
    terminate: () => {
      runtime.onmessage = null;
    },
    postMessage: (message: HostToPlugin) => {
      const copy = structuredClone(message);
      queueMicrotask(() => runtime.onmessage?.({ data: copy }));
    },
  };
  runtime.postMessage = (message: PluginToHost) => {
    const copy = structuredClone(message);
    queueMicrotask(() => port.onmessage?.({ data: copy }));
  };
  const load = MODULES[plugin.id];
  runPlugin(runtime, () => (load ? load() : Promise.reject(new Error("missing"))));
  return port;
}

function recorder() {
  const items: SidebarEntry[] = [];
  const notes: { plugin: string; message: string; gameId: string | null }[] = [];
  const failures: string[] = [];
  const ui: PluginUi = {
    setSidebarItems: (pluginId, next) => {
      items.splice(0, items.length, ...items.filter((i) => i.pluginId !== pluginId), ...next);
    },
    notify: (plugin, message, gameId) => notes.push({ plugin, message, gameId }),
    failed: (plugin, message) => failures.push(`${plugin}: ${message}`),
  };
  return { ui, items, notes, failures };
}

function active(id: string, granted: PluginInfo["granted"]): PluginInfo {
  return fakePlugin(id, { name: id, permissions: granted, granted, status: "active", enabled: true, entryUrl: `cove-plugin://localhost/${id}/index.js` });
}

let core: FakeCore;
beforeEach(() => {
  core = installFakeCore([
    fakeItem("tidewater", "Tidewater"),
    fakeItem("orbit", "Paper Orbit", { installed: false, executablePath: null }),
  ]);
});

describe("plugin host", () => {
  it("runs random-pick: sidebar item, library read, storage and a notification", async () => {
    const r = recorder();
    const plugin = active("random-pick", ["library.read", "storage", "ui.addSidebarItem", "ui.notify"]);
    core.plugins = [plugin];
    const host = new PluginHost(spawn, r.ui);
    host.sync([plugin]);
    await vi.waitFor(() => {
      expect(r.items.map((i) => i.label)).toEqual(["Was soll ich spielen?"]);
    });
    expect(r.items[0]?.icon).toBe("dice");

    host.click("random-pick", "pick", null);
    await vi.waitFor(() => {
      expect(r.notes).toHaveLength(1);
    });
    expect(r.notes[0]).toEqual({ plugin: "random-pick", message: "Wie wäre es mit „Tidewater“?", gameId: "tidewater" });
    expect(core.pluginStorage["random-pick"]).toEqual({ lastPick: "tidewater" });
  });

  it("refuses calls without permission", async () => {
    const r = recorder();
    const plugin = active("sneaky", ["ui.notify"]);
    core.plugins = [plugin];
    new PluginHost(spawn, r.ui).sync([plugin]);
    await vi.waitFor(() => {
      expect(r.notes.map((n) => n.message)).toEqual(["denied: plugin"]);
    });
    expect(core.calls.some((c) => c.command === "library_list")).toBe(false);
  });

  it("playtime-goals defines its achievements through the core", async () => {
    const r = recorder();
    const plugin = active("playtime-goals", ["achievements.define"]);
    core.plugins = [plugin];
    new PluginHost(spawn, r.ui).sync([plugin]);
    await vi.waitFor(() => {
      expect(core.pluginAchievements["playtime-goals"]).toHaveLength(5);
    });
    expect(r.failures).toEqual([]);
  });

  it("csv-import adds the games of a picked file", async () => {
    const r = recorder();
    const plugin = active("csv-import", ["library.add", "ui.addSidebarItem", "ui.notify"]);
    core.plugins = [plugin];
    const host = new PluginHost(spawn, r.ui);
    host.sync([plugin]);
    await vi.waitFor(() => {
      expect(r.items[0]?.pickFile).toEqual({ accept: ".csv,text/csv", maxBytes: 5 * 1024 * 1024 });
    });
    host.click("csv-import", "import", { name: "spiele.csv", text: "Titel;Pfad\nHollow Lantern;\nTidewater;\n" });
    await vi.waitFor(() => {
      expect(r.notes).toHaveLength(1);
    });
    expect(r.notes[0]?.message).toBe("1 Spiele importiert, 1 übersprungen (schon vorhanden oder Pfad ungültig).");
    expect(core.library.map((i) => i.game.title)).toContain("Hollow Lantern");
  });

  it("reports broken plugins and stops plugins that were switched off", async () => {
    const r = recorder();
    const broken = active("broken", []);
    const pick = active("random-pick", ["library.read", "storage", "ui.addSidebarItem", "ui.notify"]);
    core.plugins = [broken, pick];
    const host = new PluginHost(spawn, r.ui);
    host.sync([broken, pick]);
    await vi.waitFor(() => {
      expect(r.failures).toEqual(["broken: Das Plugin exportiert keine Funktion „activate“."]);
    });
    expect(host.isRunning("broken")).toBe(false);
    await vi.waitFor(() => {
      expect(r.items).toHaveLength(1);
    });
    host.sync([{ ...pick, status: "disabled", enabled: false, entryUrl: null }]);
    expect(host.isRunning("random-pick")).toBe(false);
    expect(r.items).toEqual([]);
  });
});
