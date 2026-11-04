import assert from "node:assert/strict";
import { test } from "node:test";
import { activate, parseCsv, rowsToGames } from "./index.js";

test("parses quotes, doubled quotes, line breaks and both delimiters", () => {
  assert.deepEqual(parseCsv('Titel;Pfad\r\n"Hollow; Lantern";/g/hl\n"Say ""hi""";\n'), [
    ["Titel", "Pfad"],
    ["Hollow; Lantern", "/g/hl"],
    ['Say "hi"', ""],
  ]);
  assert.deepEqual(parseCsv("\uFEFFname,path\nA,/a\n\n"), [["name", "path"], ["A", "/a"]]);
  assert.deepEqual(parseCsv('title\n"multi\nline"'), [["title"], ["multi\nline"]]);
});

test("finds columns by name, skips duplicates, works without header", () => {
  assert.deepEqual(rowsToGames([["Pfad", "Titel"], ["/g/a", "A"], ["", "B"], ["", "a"]]), [
    { title: "A", executablePath: "/g/a" },
    { title: "B", executablePath: null },
  ]);
  assert.deepEqual(rowsToGames([["Tidewater", "/g/t"], ["Paper Orbit"]]), [
    { title: "Tidewater", executablePath: "/g/t" },
    { title: "Paper Orbit", executablePath: null },
  ]);
});

test("imports the picked file and reports the result", async () => {
  /** @type {any} */
  let item;
  /** @type {string[]} */
  const added = [];
  /** @type {string[]} */
  const notes = [];
  await activate(/** @type {any} */ ({
    ui: { addSidebarItem: async (i) => { item = i; }, notify: async (m) => { notes.push(m); } },
    library: {
      add: async (game) => {
        if (game.title === "Exists") throw new Error("already");
        added.push(game.title);
        return game;
      },
    },
  }));
  assert.deepEqual(item.pickFile, { accept: ".csv,text/csv" });
  await item.onClick({ file: { name: "games.csv", text: "Titel\nA\nExists\nB\n" } });
  assert.deepEqual(added, ["A", "B"]);
  assert.equal(notes[0], "2 Spiele importiert, 1 übersprungen (schon vorhanden oder Pfad ungültig).");
  await item.onClick({ file: { name: "empty.csv", text: "\n\n" } });
  assert.match(notes[1] ?? "", /keine Spiele/);
});
