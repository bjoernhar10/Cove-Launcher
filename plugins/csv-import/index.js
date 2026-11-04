/** @typedef {import("@cove/shared/plugin-api").CoveApi} CoveApi */

const TITLE_COLUMNS = ["title", "titel", "name", "spiel", "game"];
const PATH_COLUMNS = ["path", "pfad", "executable", "executablepath", "exe"];
const MAX_ROWS = 1000;

/**
 * RFC 4180 style CSV: quoted fields, doubled quotes, line breaks inside
 * quotes. The delimiter is `;` (German spreadsheets) or `,`, whichever the
 * first line uses more.
 * @param {string} text
 * @returns {string[][]}
 */
export function parseCsv(text) {
  const source = text.replace(/^\uFEFF/, "");
  const firstLine = source.split(/\r?\n/, 1)[0] ?? "";
  const delimiter = (firstLine.match(/;/g)?.length ?? 0) > (firstLine.match(/,/g)?.length ?? 0) ? ";" : ",";
  /** @type {string[][]} */
  const rows = [];
  /** @type {string[]} */
  let row = [];
  let field = "";
  let quoted = false;
  for (let i = 0; i < source.length; i += 1) {
    const c = source[i];
    if (quoted) {
      if (c === '"' && source[i + 1] === '"') {
        field += '"';
        i += 1;
      } else if (c === '"') {
        quoted = false;
      } else {
        field += c;
      }
    } else if (c === '"' && field === "") {
      quoted = true;
    } else if (c === delimiter) {
      row.push(field);
      field = "";
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && source[i + 1] === "\n") i += 1;
      row.push(field);
      rows.push(row);
      row = [];
      field = "";
    } else {
      field += c;
    }
  }
  if (field !== "" || row.length > 0) {
    row.push(field);
    rows.push(row);
  }
  return rows.filter((r) => r.some((cell) => cell.trim() !== ""));
}

/**
 * Turns CSV rows into games. With a header row the columns are found by name,
 * otherwise column 1 is the title and column 2 the path.
 * @param {string[][]} rows
 * @returns {{ title: string, executablePath: string | null }[]}
 */
export function rowsToGames(rows) {
  const header = (rows[0] ?? []).map((h) => h.trim().toLowerCase().replace(/[^a-z]/g, ""));
  const titleIndex = header.findIndex((h) => TITLE_COLUMNS.includes(h));
  const hasHeader = titleIndex !== -1;
  const pathIndex = hasHeader ? header.findIndex((h) => PATH_COLUMNS.includes(h)) : 1;
  const body = hasHeader ? rows.slice(1) : rows;
  const seen = new Set();
  /** @type {{ title: string, executablePath: string | null }[]} */
  const games = [];
  for (const row of body.slice(0, MAX_ROWS)) {
    const title = (row[hasHeader ? titleIndex : 0] ?? "").trim();
    if (!title || seen.has(title.toLowerCase())) continue;
    seen.add(title.toLowerCase());
    const path = pathIndex >= 0 ? (row[pathIndex] ?? "").trim() : "";
    games.push({ title: title.slice(0, 200), executablePath: path || null });
  }
  return games;
}

/** @param {CoveApi} cove */
export async function activate(cove) {
  await cove.ui.addSidebarItem({
    id: "import",
    label: "CSV importieren",
    icon: "upload",
    pickFile: { accept: ".csv,text/csv" },
    onClick: async ({ file }) => {
      if (!file) return;
      const games = rowsToGames(parseCsv(file.text));
      if (games.length === 0) {
        await cove.ui.notify(`In „${file.name}“ stehen keine Spiele. Erwartet wird eine Spalte „Titel“.`);
        return;
      }
      let added = 0;
      let skipped = 0;
      for (const game of games) {
        try {
          await cove.library.add(game);
          added += 1;
        } catch {
          // Already in the library or the file path does not exist.
          skipped += 1;
        }
      }
      await cove.ui.notify(
        skipped > 0
          ? `${String(added)} Spiele importiert, ${String(skipped)} übersprungen (schon vorhanden oder Pfad ungültig).`
          : `${String(added)} Spiele importiert.`,
      );
    },
  });
}
