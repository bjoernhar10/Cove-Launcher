import { ChevronLeft, ChevronRight } from "lucide-react";
import { useRef, type ReactNode } from "react";
import { Button } from "../ui/Button.tsx";
import { Section } from "../ui/Page.tsx";

interface ScrollRowProps {
  title: string;
  description?: string | undefined;
  children: ReactNode;
  extraActions?: ReactNode;
}

/** Horizontal row of fixed-width items with previous/next buttons. */
export function ScrollRow({ title, description, children, extraActions }: ScrollRowProps) {
  const scroller = useRef<HTMLDivElement>(null);

  function scroll(direction: 1 | -1): void {
    const el = scroller.current;
    if (!el) return;
    const reduce = globalThis.matchMedia("(prefers-reduced-motion: reduce)").matches;
    el.scrollBy({ left: direction * el.clientWidth * 0.8, behavior: reduce ? "auto" : "smooth" });
  }

  return (
    <Section
      title={title}
      {...(description ? { description } : {})}
      actions={
        <>
          {extraActions}
          <Button variant="ghost" size="sm" iconOnly icon={ChevronLeft} label="Zurückblättern" onClick={() => { scroll(-1); }} />
          <Button variant="ghost" size="sm" iconOnly icon={ChevronRight} label="Weiterblättern" onClick={() => { scroll(1); }} />
        </>
      }
    >
      <div ref={scroller} className="-mx-1 flex snap-x gap-5 overflow-x-auto px-1 pb-2 [scrollbar-width:none]">
        {children}
      </div>
    </Section>
  );
}

/** Fixed-width slot inside a ScrollRow. */
export function ScrollItem({ children }: { children: ReactNode }) {
  return <div className="w-40 shrink-0 snap-start">{children}</div>;
}
