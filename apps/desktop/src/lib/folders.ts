/** Windows names that can't be used as a folder name. */
const RESERVED = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/i;

/**
 * The folder Cove creates for a game: "Half-Life 2: Episode One" → "Half-Life 2 Episode One".
 * Mirrors `folders::folder_name` in the Rust core (shown in the install dialog).
 */
export function folderName(title: string): string {
  const kept = title
    .replaceAll("&", " and ")
    .replace(/\s/gu, " ")
    .replace(/[^\p{L}\p{N} \-_.'()]/gu, "");
  const collapsed = kept.split(" ").filter(Boolean).join(" ");
  // At most 80 characters (code points, like Rust's `chars()`).
  const name = (/^.{0,80}/su.exec(collapsed)?.[0] ?? "").replace(/^[. ]+|[. ]+$/g, "");
  if (!name) return "Game";
  return RESERVED.test(name) ? `${name} Game` : name;
}

/** `folder` + `name` with the folder's own separator. */
export function joinPath(folder: string, name: string): string {
  const separator = folder.includes("\\") && !folder.includes("/") ? "\\" : "/";
  return folder.endsWith(separator) ? `${folder}${name}` : `${folder}${separator}${name}`;
}

/** Archives are unpacked, so their size after installing is only known afterwards. */
export function isArchive(fileName: string): boolean {
  return /\.(zip|7z|tar|tar\.gz|tgz)$/i.test(fileName);
}
