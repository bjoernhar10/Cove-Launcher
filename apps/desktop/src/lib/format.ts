const LOCALE = "de-DE";

const integer = new Intl.NumberFormat(LOCALE, { maximumFractionDigits: 0 });
const oneDecimal = new Intl.NumberFormat(LOCALE, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
const relative = new Intl.RelativeTimeFormat(LOCALE, { numeric: "auto" });
const dateLong = new Intl.DateTimeFormat(LOCALE, { day: "numeric", month: "long", year: "numeric" });
const dateShort = new Intl.DateTimeFormat(LOCALE, { day: "numeric", month: "short" });
const weekday = new Intl.DateTimeFormat(LOCALE, { weekday: "short" });

/** 0 → "Noch nie gespielt", 45 → "45 Min.", 754 → "12,6 Std." */
export function formatPlaytime(minutes: number): string {
  if (minutes <= 0) return "Noch nie gespielt";
  if (minutes < 60) return `${integer.format(minutes)} Min.`;
  const hours = minutes / 60;
  return `${hours < 10 ? oneDecimal.format(hours) : integer.format(hours)} Std.`;
}

/** Compact form for charts: 90 → "1,5 h", 30 → "30 min" */
export function formatMinutesShort(minutes: number): string {
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
    if (Math.abs(seconds) >= size) return relative.format(Math.round(seconds / size), unit);
  }
  return "gerade eben";
}

export function formatDate(iso: string): string {
  return dateLong.format(new Date(iso));
}

export function formatDateShort(iso: string): string {
  return dateShort.format(new Date(iso));
}

export function formatWeekday(iso: string): string {
  return weekday.format(new Date(iso));
}

export function formatYear(iso: string): string {
  return iso.slice(0, 4);
}

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** 1536 → "1,5 KB" (decimal units, like stores display them) */
export function formatBytes(bytes: number): string {
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
  if (seconds < 60) return `${integer.format(Math.ceil(seconds))} Sek.`;
  const minutes = Math.ceil(seconds / 60);
  if (minutes < 60) return `${integer.format(minutes)} Min.`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m === 0 ? `${integer.format(h)} Std.` : `${integer.format(h)} Std. ${integer.format(m)} Min.`;
}

export function formatPercent(fraction: number): string {
  return `${integer.format(Math.round(fraction * 100))} %`;
}

export function formatNumber(value: number): string {
  return integer.format(value);
}
