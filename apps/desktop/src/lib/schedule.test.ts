import { describe, expect, it } from "vitest";
import { scheduleOpen } from "./schedule.ts";

const at = (hours: number, minutes = 0) => new Date(2026, 10, 16, hours, minutes);

describe("scheduleOpen", () => {
  it("knows windows within a day and over midnight", () => {
    const night = { enabled: true, from: "01:00", until: "07:00" };
    expect(scheduleOpen(night, at(0, 59))).toBe(false);
    expect(scheduleOpen(night, at(1))).toBe(true);
    expect(scheduleOpen(night, at(7))).toBe(false);
    const late = { enabled: true, from: "23:00", until: "06:30" };
    expect(scheduleOpen(late, at(23, 30))).toBe(true);
    expect(scheduleOpen(late, at(3))).toBe(true);
    expect(scheduleOpen(late, at(12))).toBe(false);
    expect(scheduleOpen({ ...late, enabled: false }, at(12))).toBe(true);
    expect(scheduleOpen({ enabled: true, from: "08:00", until: "08:00" }, at(3))).toBe(true);
  });
});
