/**
 * Focus by direction for controllers: "right" moves to the nearest focusable
 * element to the right, by position on screen. Works for grids, rows and
 * menus without per-page code. An open dialog keeps the focus inside.
 */

export type Direction = "up" | "down" | "left" | "right";

export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Index of the best candidate in `direction` from `from`, or -1. */
export function pick(from: Box, candidates: Box[], direction: Direction): number {
  const fx = (from.left + from.right) / 2;
  const fy = (from.top + from.bottom) / 2;
  let best = -1;
  let bestScore = Infinity;
  candidates.forEach((box, i) => {
    const x = (box.left + box.right) / 2;
    const y = (box.top + box.bottom) / 2;
    // Only boxes that lie in that direction (a small overlap is allowed).
    const ahead =
      direction === "right" ? box.left >= from.right - 4 && x > fx
      : direction === "left" ? box.right <= from.left + 4 && x < fx
      : direction === "down" ? box.top >= from.bottom - 4 && y > fy
      : box.bottom <= from.top + 4 && y < fy;
    if (!ahead) return;
    const along = direction === "left" || direction === "right" ? Math.abs(x - fx) : Math.abs(y - fy);
    // Overlapping in the other axis (same row or column) counts as no offset.
    const overlap =
      direction === "left" || direction === "right"
        ? Math.min(box.bottom, from.bottom) - Math.max(box.top, from.top)
        : Math.min(box.right, from.right) - Math.max(box.left, from.left);
    const across = overlap > 0 ? 0 : direction === "left" || direction === "right" ? Math.abs(y - fy) : Math.abs(x - fx);
    const score = along + across * 3;
    if (score < bestScore) {
      bestScore = score;
      best = i;
    }
  });
  return best;
}

function visible(el: HTMLElement): boolean {
  const rect = el.getBoundingClientRect();
  return rect.width > 0 && rect.height > 0 && getComputedStyle(el).visibility !== "hidden";
}

/** Where focus may go: the open dialog, else the page. */
function scope(): HTMLElement {
  const dialogs = [...document.querySelectorAll<HTMLDialogElement>("dialog[open]")];
  return dialogs.at(-1) ?? document.body;
}

export function focusables(root: HTMLElement = scope()): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => !el.closest("[inert], [aria-hidden=true]") && visible(el));
}

function show(el: HTMLElement): void {
  el.focus({ preventScroll: true });
  const reduce = globalThis.matchMedia("(prefers-reduced-motion: reduce)").matches;
  el.scrollIntoView({ block: "nearest", inline: "nearest", behavior: reduce ? "auto" : "smooth" });
}

/** Moves the focus; with nothing focused yet, the page's default (or first) element. */
export function moveFocus(direction: Direction): void {
  const root = scope();
  const all = focusables(root);
  const current = document.activeElement instanceof HTMLElement && root.contains(document.activeElement) && document.activeElement !== document.body ? document.activeElement : null;
  if (!current) {
    const start = root.querySelector<HTMLElement>("[data-nav-default]") ?? all[0];
    if (start) show(start);
    return;
  }
  const others = all.filter((el) => el !== current);
  const index = pick(current.getBoundingClientRect(), others.map((el) => el.getBoundingClientRect()), direction);
  const next = others[index];
  if (next) show(next);
}
