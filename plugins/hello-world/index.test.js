import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { activate } from "./index.js";

test("activate logs and returns a greeting", () => {
  const logs = [];
  const result = activate({ log: (m) => logs.push(m) });
  assert.equal(logs.length, 1);
  assert.equal(result.greeting, logs[0]);
});

test("manifest points at an existing entry", async () => {
  const manifest = JSON.parse(await readFile(new URL("./manifest.json", import.meta.url), "utf8"));
  assert.equal(manifest.id, "hello-world");
  assert.equal(manifest.entry, "index.js");
});
