import type { ReactNode } from "react";
import { cx } from "../../lib/cx.ts";

export function Page({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cx("mx-auto w-full max-w-[1440px] px-8 pt-8 pb-16", className)}>{children}</div>;
}

interface PageHeaderProps {
  title: string;
  description?: ReactNode;
  actions?: ReactNode;
}

export function PageHeader({ title, description, actions }: PageHeaderProps) {
  return (
    <header className="mb-8 flex items-end justify-between gap-6">
      <div className="min-w-0">
        <h1 className="text-xl font-semibold tracking-tight text-fg">{title}</h1>
        {description && <p className="mt-1 text-sm text-fg-muted">{description}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </header>
  );
}

interface SectionProps {
  title: string;
  description?: string | undefined;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}

export function Section({ title, description, actions, children, className }: SectionProps) {
  return (
    <section className={cx("mt-12 first:mt-0", className)} aria-label={title}>
      <div className="mb-4 flex items-end justify-between gap-4">
        <div>
          <h2 className="text-base font-semibold text-fg">{title}</h2>
          {description && <p className="mt-0.5 text-sm text-fg-muted">{description}</p>}
        </div>
        {actions && <div className="flex items-center gap-1">{actions}</div>}
      </div>
      {children}
    </section>
  );
}
