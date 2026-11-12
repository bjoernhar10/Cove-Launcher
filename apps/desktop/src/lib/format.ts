import { locale, messages } from "../i18n/index.ts";

interface Formatters {
  integer: Intl.NumberFormat;
  oneDecimal: Intl.NumberFormat;
  relative: Intl.RelativeTimeFormat;
  dateLong: Intl.DateTimeFormat;
  dateShort: Intl.DateTimeFormat;
  weekday: Intl.DateTimeFormat;
}

const cache = new Map<string, Formatters>();

/** `Intl` formatters for the current UI language (built once per language). */
function fmt(): Formatters {
  const tag = locale();
  let f = cache.get(tag);
  if (!f) {
    f = {
      integer: new Intl.NumberFormat(tag, { maximumFractionDigits: 0 }),
      oneDecimal: new Intl.NumberFormat(tag, { minimumFractionDigits: 1, maximumFractionDigits: 1 }),
      relative: new Intl.RelativeTimeFormat(tag, { numeric: "auto" }),
      dateLong: new Intl.DateTimeFormat(tag, { day: "numeric", month: "long", year: "numeric" }),
      dateShort: new Intl.DateTimeFormat(tag, { day: "numeric", month: "short" }),
      weekday: new Intl.DateTimeFormat(tag, { weekday: "short" }),
    };
    cache.set(tag, f);
  }
  return f;
}

/** 0 → "Noch nie gespielt", 45 → "45 Min.", 450 → "7,5 Std." (German UI) */
export function formatPlaytime(minutes: number): string {
  const { integer, oneDecimal } = fmt();
  const u = messages().units;
  if (minutes <= 0) return u.neverPlayed;
  if (minutes < 60) return `${integer.format(minutes)} ${u.minutes}`;
  const hours = minutes / 60;
  return `${hours < 10 ? oneDecimal.format(hours) : integer.format(hours)} ${u.hours}`;
}

/** A game's length from seconds, rounded like "about": 36 000 → "10 Std.", 5400 → "1,5 Std.", 1200 → "20 Min." */
export function formatHours(seconds: number): string {
  const { integer, oneDecimal } = fmt();
  const u = messages().units;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${integer.format(Math.max(1, minutes))} ${u.minutes}`;
  // Half hours below 10 h, whole hours above: the reports are rough anyway.
  const hours = seconds < 36_000 ? Math.round(seconds / 1800) / 2 : Math.round(seconds / 3600);
  return `${Number.isInteger(hours) ? integer.format(hours) : oneDecimal.format(hours)} ${u.hours}`;
}

/** Compact form for charts: 90 → "1,5 h", 30 → "30 min" */
export function formatMinutesShort(minutes: number): string {
  const { integer, oneDecimal } = fmt();
  if (minutes < 60) return `${integer.format(minutes)} min`;
  return `${oneDecimal.format(minutes / 60)} h`;
}

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 24 * 3600],
  ["month", 30 * 24 * 3600],
  ["week", 7 * 24 * 3600],
  ["day", 24 * 3600],
  ["hour", 3600],
  ["minute", 60],
];

/** "vor 2 Tagen", "gestern", "gerade eben" */
export function formatRelative(iso: string, now: Date = new Date()): string {
  const seconds = (new Date(iso).getTime() - now.getTime()) / 1000;
  for (const [unit, size] of UNITS) {
    if (Math.abs(seconds) >= size) return fmt().relative.format(Math.round(seconds / size), unit);
  }
  return messages().units.justNow;
}

export function formatDate(iso: string): string {
  return fmt().dateLong.format(new Date(iso));
}

export function formatDateShort(iso: string): string {
  return fmt().dateShort.format(new Date(iso));
}

export function formatWeekday(iso: string): string {
  return fmt().weekday.format(new Date(iso));
}

export function formatYear(iso: string): string {
  return iso.slice(0, 4);
}

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** 1536 → "1,5 KB" (decimal units, like stores display them) */
export function formatBytes(bytes: number): string {
  const { integer, oneDecimal } = fmt();
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < BYTE_UNITS.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const formatted = unit === 0 || value >= 100 ? integer.format(value) : oneDecimal.format(value);
  return `${formatted} ${BYTE_UNITS[unit] ?? "B"}`;
}

export function formatSpeed(bytesPerSecond: number): string {
  return `${formatBytes(bytesPerSecond)}/s`;
}

/** Remaining time: "45 Sek.", "12 Min.", "1 Std. 5 Min." */
export function formatEta(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "—";
  const { integer } = fmt();
  const u = messages().units;
  if (seconds < 60) return `${integer.format(Math.ceil(seconds))} ${u.seconds}`;
  const minutes = Math.ceil(seconds / 60);
  if (minutes < 60) return `${integer.format(minutes)} ${u.minutes}`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m === 0 ? `${integer.format(h)} ${u.hours}` : `${integer.format(h)} ${u.hours} ${integer.format(m)} ${u.minutes}`;
}

export function formatPercent(fraction: number): string {
  return messages().units.percent(fmt().integer.format(Math.round(fraction * 100)));
}

export function formatNumber(value: number): string {
  return fmt().integer.format(value);
}
