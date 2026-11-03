import { X } from "lucide-react";
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
          <p className="min-w-0 flex-1 pt-0.5">{t.message}</p>
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
