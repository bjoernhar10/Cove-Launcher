import { useRef, type ReactNode } from "react";
import { columnsFor, useElementWidth, useMainVirtualizer, VIRTUALIZE_FROM } from "../../hooks/useMainVirtualizer.ts";
import { COVER_GRID, COVER_GRID_GAP, COVER_GRID_MIN } from "./GameCard.tsx";

export interface GridCell {
  key: string;
  node: ReactNode;
}

interface VirtualGridProps {
  cells: readonly GridCell[];
  /** Accessible name of the list. */
  label: string;
}

/**
 * The cover grid of Catalogue and Library. Short lists render as a plain CSS
 * grid; long ones (thousands of games) only render the rows near the viewport.
 */
export function VirtualGrid({ cells, label }: VirtualGridProps) {
  if (cells.length <= VIRTUALIZE_FROM) {
    return (
      <ul className={COVER_GRID} aria-label={label}>
        {cells.map((cell) => (
          <li key={cell.key}>{cell.node}</li>
        ))}
      </ul>
    );
  }
  return <VirtualRows cells={cells} label={label} />;
}

function VirtualRows({ cells, label }: VirtualGridProps) {
  const listRef = useRef<HTMLDivElement>(null);
  const width = useElementWidth(listRef);
  const columns = columnsFor(width, COVER_GRID_MIN, COVER_GRID_GAP.x);
  const columnWidth = (Math.max(width, COVER_GRID_MIN) - (columns - 1) * COVER_GRID_GAP.x) / columns;
  const virtualizer = useMainVirtualizer({
    listRef,
    count: Math.ceil(cells.length / columns),
    // Portrait cover plus two text lines; real heights are measured.
    estimateSize: Math.round((columnWidth * 4) / 3 + 64),
    gap: COVER_GRID_GAP.y,
  });
  const margin = virtualizer.options.scrollMargin;

  return (
    <div ref={listRef} role="list" aria-label={label} className="relative" style={{ height: virtualizer.getTotalSize() }}>
      {virtualizer.getVirtualItems().map((row) => {
        const first = row.index * columns;
        return (
          <div
            key={row.key}
            ref={virtualizer.measureElement}
            data-index={row.index}
            role="none"
            className="absolute inset-x-0 top-0 grid"
            style={{
              transform: `translateY(${String(row.start - margin)}px)`,
              gridTemplateColumns: `repeat(${String(columns)}, minmax(0, 1fr))`,
              columnGap: COVER_GRID_GAP.x,
            }}
          >
            {cells.slice(first, first + columns).map((cell, i) => (
              <div key={cell.key} role="listitem" aria-posinset={first + i + 1} aria-setsize={cells.length}>
                {cell.node}
              </div>
            ))}
          </div>
        );
      })}
    </div>
  );
}
