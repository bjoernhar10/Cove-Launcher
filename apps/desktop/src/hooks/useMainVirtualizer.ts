import { useVirtualizer, type Virtualizer } from "@tanstack/react-virtual";
import { useLayoutEffect, useState, type RefObject } from "react";

/** Lists up to this length render plainly; longer ones are virtualized. */
export const VIRTUALIZE_FROM = 60;

/** The app's scroll container (`<main id="main">` in `AppShell`). */
export function mainScrollElement(): HTMLElement | null {
  return document.getElementById("main");
}

/**
 * Distance from the top of the scroll container's content to `element`.
 * TanStack Virtual needs it as `scrollMargin` because the list does not start at 0.
 */
function offsetInMain(element: HTMLElement): number {
  const main = mainScrollElement();
  if (!main) return 0;
  return Math.round(element.getBoundingClientRect().top - main.getBoundingClientRect().top + main.scrollTop);
}

interface Options<E extends HTMLElement> {
  /** The element that contains the rows. */
  listRef: RefObject<E | null>;
  count: number;
  estimateSize: number;
  /** Space between rows in px. */
  gap?: number;
  /** Rows rendered above and below the viewport. */
  overscan?: number;
}

/**
 * A row virtualizer bound to `#main`. Row `start` values are relative to
 * the scroll content, so subtract `virtualizer.options.scrollMargin` when positioning.
 */
export function useMainVirtualizer<E extends HTMLElement>({
  listRef,
  count,
  estimateSize,
  gap = 0,
  overscan = 4,
}: Options<E>): Virtualizer<HTMLElement, Element> {
  const [scrollMargin, setScrollMargin] = useState(0);

  // Content above the list can change height (filters wrap, banners appear):
  // re-measure whenever the page content resizes.
  useLayoutEffect(() => {
    const element = listRef.current;
    if (!element) return;
    const measure = () => {
      setScrollMargin(offsetInMain(element));
    };
    measure();
    const content = mainScrollElement()?.firstElementChild;
    if (!content || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(content);
    return () => {
      observer.disconnect();
    };
  }, [listRef]);

  // TanStack Virtual returns fresh functions on purpose; nothing downstream memoizes them.
  // eslint-disable-next-line react-hooks/incompatible-library
  return useVirtualizer<HTMLElement, Element>({
    count,
    getScrollElement: mainScrollElement,
    estimateSize: () => estimateSize,
    gap,
    overscan,
    scrollMargin,
  });
}

/** Number of columns a CSS `repeat(auto-fill, minmax(min, 1fr))` grid would use. */
export function columnsFor(width: number, minColumn: number, gap: number): number {
  return Math.max(1, Math.floor((width + gap) / (minColumn + gap)));
}

/** Width of an element, kept up to date with a ResizeObserver. */
export function useElementWidth(ref: RefObject<HTMLElement | null>): number {
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    setWidth(element.clientWidth);
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) setWidth(Math.round(entry.contentRect.width));
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, [ref]);
  return width;
}
