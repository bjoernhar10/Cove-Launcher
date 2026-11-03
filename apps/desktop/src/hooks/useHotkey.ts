import { useEffect, useRef } from "react";

export interface HotkeyOptions {
  mod?: boolean;
  alt?: boolean;
  shift?: boolean;
  /** Also fire while typing in inputs (default true for mod-combos, false otherwise). */
  allowInInputs?: boolean;
}

function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

/** Global keyboard shortcut. `mod` = Ctrl (Linux/Windows) or ⌘ (macOS). */
export function useHotkey(key: string, handler: (event: KeyboardEvent) => void, options: HotkeyOptions = {}): void {
  const handlerRef = useRef(handler);
  useEffect(() => {
    handlerRef.current = handler;
  });
  const { mod = false, alt = false, shift = false } = options;
  const allowInInputs = options.allowInInputs ?? mod;

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent): void {
      const modPressed = event.ctrlKey || event.metaKey;
      if (event.key.toLowerCase() !== key.toLowerCase()) return;
      if (modPressed !== mod || event.altKey !== alt || event.shiftKey !== shift) return;
      if (!allowInInputs && isTyping(event.target)) return;
      event.preventDefault();
      handlerRef.current(event);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [key, mod, alt, shift, allowInInputs]);
}
