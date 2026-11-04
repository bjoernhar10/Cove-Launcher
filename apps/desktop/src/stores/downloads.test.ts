import { describe, expect, it } from "vitest";
import { fakeDownload } from "../test/fakeCore.ts";
import { applyProgress, selectActive, selectRunning } from "./downloads.ts";

describe("download store", () => {
  it("applies progress only to the matching running download", () => {
    const items = [
      fakeDownload("a", "A", { status: "downloading", totalBytes: 100 }),
      fakeDownload("b", "B", { status: "paused", totalBytes: 100, downloadedBytes: 10 }),
    ];
    const next = applyProgress(items, { id: "a", status: "downloading", downloadedBytes: 40, totalBytes: 100, bytesPerSecond: 20 });
    expect(next[0]?.download).toMatchObject({ downloadedBytes: 40, bytesPerSecond: 20 });
    // A late event for a paused download does not overwrite it.
    const late = applyProgress(next, { id: "b", status: "downloading", downloadedBytes: 99, totalBytes: 100, bytesPerSecond: 5 });
    expect(late[1]?.download.downloadedBytes).toBe(10);
  });

  it("groups statuses", () => {
    const items = [
      fakeDownload("h", "Store", { status: "handedOff", origin: "steam" }),
      fakeDownload("x", "Unpack", { status: "extracting" }),
      fakeDownload("q", "Queued", { status: "queued" }),
    ];
    expect(selectActive(items).map((i) => i.download.id)).toEqual(["h", "x"]);
    expect(selectRunning(items)?.download.id).toBe("x");
  });
});
