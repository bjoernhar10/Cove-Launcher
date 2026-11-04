import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { activate, goalAchievements, GOALS } from "./index.js";

test("defines one playtime achievement per goal", async () => {
  /** @type {unknown[]} */
  let defined = [];
  await activate(/** @type {any} */ ({ achievements: { define: async (list) => { defined = list; return list.length; } } }));
  assert.equal(defined.length, GOALS.length);
  assert.deepEqual(defined[0], {
    id: "hours-5",
    title: "Warmgespielt",
    description: "Spiele insgesamt 5 Stunden über Cove.",
    rule: { type: "playtimeHours", hours: 5 },
  });
});

test("ids are unique and valid", () => {
  const ids = goalAchievements().map((a) => a.id);
  assert.equal(new Set(ids).size, ids.length);
  for (const id of ids) assert.match(id, /^[a-z0-9][a-z0-9-]*$/);
});

test("manifest asks only for what it uses", async () => {
  const manifest = JSON.parse(await readFile(new URL("./plugin.json", import.meta.url), "utf8"));
  assert.deepEqual(manifest.permissions, ["achievements.define"]);
  assert.equal(manifest.entry, "index.js");
});
