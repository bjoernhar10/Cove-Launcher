import { Trophy, X } from "lucide-react";
import { Link } from "react-router";
import { cx } from "../../lib/cx.ts";
import { useToasts } from "../../stores/toasts.ts";

export function Toaster() {
  const toasts = useToasts((s) => s.toasts);
  const dismiss = useToasts((s) => s.dismiss);
  return (
    <div aria-live="polite" className="pointer-events-none fixed right-6 bottom-6 z-50 flex w-80 flex-col gap-2">
      {toasts.map((t) => (
        <div
          key={t.id}
          role={t.tone === "danger" ? "alert" : "status"}
          className={cx(
            "pointer-events-auto flex items-start gap-3 rounded-lg border border-border bg-raised py-3 pr-2 pl-4 text-sm",
            "transition-[opacity,translate] duration-200 ease-out starting:translate-y-1 starting:opacity-0",
            t.tone === "danger" ? "text-danger" : "text-fg",
          )}
        >
          {t.kind === "achievement" && <Trophy aria-hidden className="mt-1 shrink-0 text-accent-text" />}
          <div className="min-w-0 flex-1 pt-1">
            {t.title && <p className="text-xs text-fg-muted">{t.title}</p>}
            <p className={t.title ? "font-medium" : undefined}>{t.message}</p>
            {t.action && (
              <Link
                to={t.action.to}
                className="mt-1 inline-block text-sm font-medium text-accent-text hover:underline"
                onClick={() => {
                  dismiss(t.id);
                }}
              >
                {t.action.label}
              </Link>
            )}
          </div>
          <button
            type="button"
            aria-label="Hinweis schließen"
            onClick={() => {
              dismiss(t.id);
            }}
            className="rounded-md p-1 text-fg-subtle hover:bg-hover hover:text-fg"
          >
            <X aria-hidden />
          </button>
        </div>
      ))}
    </div>
  );
}
