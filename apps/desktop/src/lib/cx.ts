/** Joins class names, skipping falsy values. */
export function cx(...classes: (string | false | null | undefined)[]): string {
  return classes.filter(Boolean).join(" ");
}

/**
 * Returns `fallback` unless `className` already contains a utility starting
 * with one of `prefixes` — lets callers override e.g. the default radius
 * without two conflicting Tailwind classes on the same element.
 */
export function unlessSet(className: string | undefined, prefixes: readonly string[], fallback: string): string | null {
  const classes = className?.split(/\s+/) ?? [];
  return classes.some((c) => prefixes.some((p) => c.startsWith(p))) ? null : fallback;
}
