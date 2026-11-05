import { useMessages } from "../../i18n/index.ts";
import { X } from "lucide-react";
import { useEffect, useId, useRef, type ReactNode } from "react";
import { cx } from "../../lib/cx.ts";
import { Button } from "./Button.tsx";

interface DialogProps {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string | undefined;
  children: ReactNode;
  footer?: ReactNode;
  /** Hide the visual header (title stays the accessible name). */
  bare?: boolean;
  className?: string;
}

/**
 * Modal dialog on top of native <dialog> (focus trap, Esc, inert background
 * for free). Backdrop is a flat overlay — no blur.
 */
export function Dialog({ open, onClose, title, description, children, footer, bare = false, className }: DialogProps) {
  const t = useMessages();
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const descId = useId();

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);

  return (
    <dialog
      ref={ref}
      aria-labelledby={titleId}
      aria-describedby={description ? descId : undefined}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      onClick={(e) => {
        // Click on the backdrop (the dialog element itself) closes.
        if (e.target === e.currentTarget) onClose();
      }}
      className={cx(
        "m-auto max-h-[calc(100vh-64px)] w-[520px] max-w-[calc(100vw-32px)] overflow-hidden rounded-lg border border-border bg-raised p-0 text-fg",
        "opacity-100 transition-opacity duration-150 ease-out starting:opacity-0",
        className,
      )}
    >
      {open && (
        <div className="flex max-h-[calc(100vh-64px)] flex-col">
          {bare ? (
            <h2 id={titleId} className="sr-only">
              {title}
            </h2>
          ) : (
            <header className="flex items-start justify-between gap-4 px-6 pt-5 pb-4">
              <div>
                <h2 id={titleId} className="text-base font-semibold">
                  {title}
                </h2>
                {description && (
                  <p id={descId} className="mt-1 text-sm text-fg-muted">
                    {description}
                  </p>
                )}
              </div>
              <Button variant="ghost" size="sm" iconOnly icon={X} label={t.common.close} onClick={onClose} className="-mt-1 -mr-2" />
            </header>
          )}
          <div className={cx("min-h-0 flex-1 overflow-y-auto", !bare && "px-6 pb-6")}>{children}</div>
          {footer && <footer className="flex justify-end gap-2 border-t border-border px-6 py-4">{footer}</footer>}
        </div>
      )}
    </dialog>
  );
}
