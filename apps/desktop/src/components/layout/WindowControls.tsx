import { useMessages } from "../../i18n/index.ts";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";
import { cx } from "../../lib/cx.ts";
import { inTauri } from "../../lib/env.ts";
import { errorMessage, toast } from "../../stores/toasts.ts";

type WindowAction = "minimize" | "toggleMaximize" | "close";

function run(action: WindowAction): void {
  if (!inTauri) return;
  const win = getCurrentWindow();
  const promise = action === "minimize" ? win.minimize() : action === "toggleMaximize" ? win.toggleMaximize() : win.close();
  promise.catch((error: unknown) => {
    toast(errorMessage(error), "danger");
  });
}

const BUTTON = "inline-flex h-8 w-10 items-center justify-center rounded-md text-fg-muted transition-colors duration-150 ease-out";

/** Minimal window buttons for the custom title bar (decorations: false). */
export function WindowControls() {
  const t = useMessages();
  const hint = inTauri ? undefined : t.window.desktopOnly;
  return (
    <div className="flex items-center gap-1" role="group" aria-label={t.window.group}>
      <button type="button" aria-label={t.window.minimize} title={hint ?? t.window.minimize} className={cx(BUTTON, "hover:bg-hover hover:text-fg")} onClick={() => { run("minimize"); }}>
        <Minus aria-hidden />
      </button>
      <button type="button" aria-label={t.window.maximize} title={hint ?? t.window.maximize} className={cx(BUTTON, "hover:bg-hover hover:text-fg")} onClick={() => { run("toggleMaximize"); }}>
        <Square aria-hidden className="scale-75" />
      </button>
      <button type="button" aria-label={t.window.close} title={hint ?? t.window.close} className={cx(BUTTON, "hover:bg-danger-subtle hover:text-danger")} onClick={() => { run("close"); }}>
        <X aria-hidden />
      </button>
    </div>
  );
}
