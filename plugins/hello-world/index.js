/**
 * @typedef {object} PluginContext
 * @property {(message: string) => void} log
 */

/**
 * Called once when Cove loads the plugin.
 * @param {PluginContext} ctx
 * @returns {{ greeting: string }}
 */
export function activate(ctx) {
  const greeting = "Hello from the hello-world plugin";
  ctx.log(greeting);
  return { greeting };
}

/** Called when the plugin is unloaded. */
export function deactivate() {}
