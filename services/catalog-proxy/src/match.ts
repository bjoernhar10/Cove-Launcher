/**
 * Title similarity, same rules as the core (`catalog/matching.rs`):
 * normalized titles, 0.6 × Levenshtein + 0.4 × Jaro-Winkler, 1.0 only when identical.
 */
const EDITION_SUFFIXES = [
  "game of the year edition",
  "game of the year",
  "goty edition",
  "goty",
  "definitive edition",
  "deluxe edition",
  "complete edition",
  "enhanced edition",
  "anniversary edition",
  "digital deluxe",
  "standard edition",
];

const ROMAN: Record<string, string> = { ii: "2", iii: "3", iv: "4", v: "5", vi: "6", vii: "7", viii: "8", ix: "9", x: "10" };

export function normalize(title: string): string {
  let text = title
    .toLowerCase()
    .replace(/&/g, " and ")
    .replace(/[™®©'’]/g, "")
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();
  for (const suffix of EDITION_SUFFIXES) {
    if (text.endsWith(suffix)) text = text.slice(0, -suffix.length).trimEnd();
  }
  return text
    .split(" ")
    .map((w) => ROMAN[w] ?? w)
    .join(" ");
}

function levenshtein(a: string, b: string): number {
  const prev = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i += 1) {
    let diag = prev[0] ?? 0;
    prev[0] = i;
    for (let j = 1; j <= b.length; j += 1) {
      const tmp = prev[j] ?? 0;
      prev[j] = Math.min((prev[j] ?? 0) + 1, (prev[j - 1] ?? 0) + 1, diag + (a[i - 1] === b[j - 1] ? 0 : 1));
      diag = tmp;
    }
  }
  return prev[b.length] ?? 0;
}

function jaroWinkler(a: string, b: string): number {
  if (a === b) return 1;
  const range = Math.max(0, Math.floor(Math.max(a.length, b.length) / 2) - 1);
  const aMatch = new Array<boolean>(a.length).fill(false);
  const bMatch = new Array<boolean>(b.length).fill(false);
  let matches = 0;
  for (let i = 0; i < a.length; i += 1) {
    for (let j = Math.max(0, i - range); j < Math.min(b.length, i + range + 1); j += 1) {
      if (!bMatch[j] && a[i] === b[j]) {
        aMatch[i] = bMatch[j] = true;
        matches += 1;
        break;
      }
    }
  }
  if (matches === 0) return 0;
  let k = 0;
  let transpositions = 0;
  for (let i = 0; i < a.length; i += 1) {
    if (!aMatch[i]) continue;
    while (!bMatch[k]) k += 1;
    if (a[i] !== b[k]) transpositions += 1;
    k += 1;
  }
  const jaro = (matches / a.length + matches / b.length + (matches - transpositions / 2) / matches) / 3;
  let prefix = 0;
  while (prefix < 4 && a[prefix] !== undefined && a[prefix] === b[prefix]) prefix += 1;
  return jaro + prefix * 0.1 * (1 - jaro);
}

export function score(a: string, b: string): number {
  const x = normalize(a);
  const y = normalize(b);
  if (!x || !y) return 0;
  if (x === y) return 1;
  const lev = 1 - levenshtein(x, y) / Math.max(x.length, y.length);
  return Math.min(0.99, 0.6 * lev + 0.4 * jaroWinkler(x, y));
}
