import type { PluginInfo } from "@cove/shared";
import type { PluginPort } from "./host.ts";
import PluginWorker from "./worker.ts?worker&inline";

/** One dedicated worker per plugin. */
export function spawnWorker(plugin: PluginInfo): PluginPort {
  const worker = new PluginWorker({ name: `plugin:${plugin.id}` });
  const port: PluginPort = {
    postMessage: (message) => {
      worker.postMessage(message);
    },
    onmessage: null,
    onerror: null,
    terminate: () => {
      worker.terminate();
    },
  };
  worker.onmessage = (event: MessageEvent) => {
    port.onmessage?.({ data: event.data as Parameters<NonNullable<PluginPort["onmessage"]>>[0]["data"] });
  };
  worker.onerror = (event: ErrorEvent) => {
    event.preventDefault();
    port.onerror?.(event.message);
  };
  return port;
}
