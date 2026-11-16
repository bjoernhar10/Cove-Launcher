import type { DownloadSchedule } from "@cove/shared";

function minutes(time: string): number | null {
  const match = /^(\d{2}):(\d{2})$/.exec(time);
  if (!match) return null;
  const [hours, mins] = [Number(match[1]), Number(match[2])];
  return hours < 24 && mins < 60 ? hours * 60 + mins : null;
}

/** Whether downloads may run at `now` (mirrors `downloads::schedule::open_at` in the core). */
export function scheduleOpen(schedule: DownloadSchedule, now: Date): boolean {
  if (!schedule.enabled) return true;
  const from = minutes(schedule.from);
  const until = minutes(schedule.until);
  if (from === null || until === null || from === until) return true;
  const current = now.getHours() * 60 + now.getMinutes();
  return from < until ? current >= from && current < until : current >= from || current < until;
}
