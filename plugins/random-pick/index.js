/** @typedef {import("@cove/shared/plugin-api").CoveApi} CoveApi */
/** @typedef {import("@cove/shared/plugin-api").PluginGame} PluginGame */

const LAST_PICK = "lastPick";

/**
 * Picks an installed game, never the same one twice in a row (if there is a
 * choice). Games not played for a while get a slightly higher chance.
 * @param {PluginGame[]} games
 * @param {string | null} lastId
 * @param {() => number} [random]
 * @param {number} [now]
 * @returns {PluginGame | null}
 */
export function pickGame(games, lastId, random = Math.random, now = Date.now()) {
  const installed = games.filter((g) => g.installed);
  const pool = installed.length > 1 ? installed.filter((g) => g.id !== lastId) : installed;
  if (pool.length === 0) return null;
  const weights = pool.map((g) => {
    if (!g.lastPlayedAt) return 2;
    const days = (now - Date.parse(g.lastPlayedAt)) / 86_400_000;
    return 1 + Math.min(1, Math.max(0, days / 30));
  });
  const total = weights.reduce((sum, w) => sum + w, 0);
  let roll = random() * total;
  for (let i = 0; i < pool.length; i += 1) {
    roll -= weights[i] ?? 0;
    if (roll < 0) return pool[i] ?? null;
  }
  return pool[pool.length - 1] ?? null;
}

/** @param {CoveApi} cove */
export async function activate(cove) {
  await cove.ui.addSidebarItem({
    id: "pick",
    label: "Was soll ich spielen?",
    icon: "dice",
    onClick: async () => {
      const games = await cove.library.list();
      const last = await cove.storage.get(LAST_PICK);
      const game = pickGame(games, typeof last === "string" ? last : null);
      if (!game) {
        await cove.ui.notify("Noch keine installierten Spiele. Installiere zuerst etwas.");
        return;
      }
      await cove.storage.set(LAST_PICK, game.id);
      await cove.ui.notify(`Wie wäre es mit „${game.title}“?`, { gameId: game.id });
    },
  });
}
