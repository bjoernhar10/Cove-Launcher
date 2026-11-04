/** @typedef {import("@cove/shared/plugin-api").CoveApi} CoveApi */
/** @typedef {import("@cove/shared/plugin-api").PluginAchievement} PluginAchievement */

/** Hours played in Cove → achievement title. */
export const GOALS = [
  { hours: 5, title: "Warmgespielt" },
  { hours: 25, title: "Dranbleiber" },
  { hours: 100, title: "Hundert Stunden" },
  { hours: 250, title: "Ausdauer" },
  { hours: 500, title: "Lebenswerk" },
];

/**
 * One achievement per goal.
 * @returns {PluginAchievement[]}
 */
export function goalAchievements() {
  return GOALS.map(({ hours, title }) => ({
    id: `hours-${String(hours)}`,
    title,
    description: `Spiele insgesamt ${String(hours)} Stunden über Cove.`,
    rule: { type: "playtimeHours", hours },
  }));
}

/** @param {CoveApi} cove */
export async function activate(cove) {
  // Defining replaces earlier definitions, so this is safe on every start.
  await cove.achievements.define(goalAchievements());
}
