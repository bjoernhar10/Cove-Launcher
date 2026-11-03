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
  const hint = inTauri ? undefined : "Nur in der Desktop-App";
  return (
    <div className="flex items-center gap-0.5" role="group" aria-label="Fenster">
      <button type="button" aria-label="Minimieren" title={hint ?? "Minimieren"} className={cx(BUTTON, "hover:bg-hover hover:text-fg")} onClick={() => { run("minimize"); }}>
        <Minus aria-hidden />
      </button>
      <button type="button" aria-label="Maximieren" title={hint ?? "Maximieren"} className={cx(BUTTON, "hover:bg-hover hover:text-fg")} onClick={() => { run("toggleMaximize"); }}>
        <Square aria-hidden className="scale-75" />
      </button>
      <button type="button" aria-label="Schließen" title={hint ?? "Schließen"} className={cx(BUTTON, "hover:bg-danger-subtle hover:text-danger")} onClick={() => { run("close"); }}>
        <X aria-hidden />
      </button>
    </div>
  );
}
