import type { TimeToBeat } from "@cove/shared";
import type { LengthFilter } from "../stores/preferences.ts";

/** Hours per band (the last band is open). */
export const LENGTHS: {
  value: Exclude<LengthFilter, "any">;
  min: number;
  max: number;
}[] = [
  { value: "short", min: 0, max: 5 },
  { value: "medium", min: 5, max: 15 },
  { value: "long", min: 15, max: 40 },
  { value: "veryLong", min: 40, max: Number.POSITIVE_INFINITY },
];

/** The length that sorts a game: main story and extras, else whatever is known (seconds). */
export function typicalLength(times: TimeToBeat): number | null {
  return times.normally ?? times.hastily ?? times.completely;
}

/** Whether a game's length falls into a band; unknown lengths only match "any". */
export function inBand(times: TimeToBeat | undefined, filter: LengthFilter): boolean {
  if (filter === "any") return true;
  const band = LENGTHS.find((l) => l.value === filter);
  const seconds = times ? typicalLength(times) : null;
  if (!band || seconds === null) return false;
  const hours = seconds / 3600;
  return hours >= band.min && hours < band.max;
}
