import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { createGenerator } from "ts-json-schema-generator";
import { describe, expect, it } from "vitest";

const root = fileURLToPath(new URL("..", import.meta.url));

describe("models.schema.json", () => {
  it("is up to date with src/models.ts (run `npm run schemas -w @cove/shared`)", () => {
    const generated = createGenerator({
      path: `${root}src/models.ts`,
      tsconfig: `${root}tsconfig.json`,
      type: "*",
      skipTypeCheck: true,
      schemaId: "https://cove.local/schemas/models.schema.json",
    }).createSchema("*");
    const committed: unknown = JSON.parse(readFileSync(`${root}schemas/models.schema.json`, "utf8"));
    expect(committed).toEqual(JSON.parse(JSON.stringify(generated)));
  }, 30_000);
});
