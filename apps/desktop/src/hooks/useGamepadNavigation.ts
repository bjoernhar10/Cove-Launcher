import type { PadAction } from "@cove/shared";
import { useEffect } from "react";
import { useLocation, useNavigate } from "react-router";
import { api } from "../api/index.ts";
import { moveFocus } from "../lib/spatialNav.ts";
import { useGamepads } from "../stores/gamepad.ts";

/** Page-level controller actions (tabs, favourite …) go out as this window event. */
export const PAD_EVENT = "cove-pad";

export function onPad(handler: (action: PadAction) => void): () => void {
  const listener = (event: Event) => {
    if (event instanceof CustomEvent && typeof event.detail === "string") handler(event.detail as PadAction);
  };
  window.addEventListener(PAD_EVENT, listener);
  return () => {
    window.removeEventListener(PAD_EVENT, listener);
  };
}

function typing(el: Element | null): boolean {
  return el instanceof HTMLElement && (el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName));
}

/**
 * Controllers move the focus everywhere in Cove: the D-pad and the stick by
 * direction, the bottom button presses, the right one goes back (or closes a
 * dialog). The Guide button opens the TV mode. The focus ring shows while the
 * controller is in use.
 */
export function useGamepadNavigation(): void {
  const navigate = useNavigate();
  const location = useLocation();
  const tv = location.pathname.startsWith("/tv");

  useEffect(() => {
    const root = document.documentElement;
    const usingMouse = () => {
      delete root.dataset.padNav;
    };
    window.addEventListener("pointermove", usingMouse);
    const stop = api.gamepad.subscribeInput(({ action, kind }) => {
      useGamepads.getState().setKind(kind);
      root.dataset.padNav = "true";
      const active = document.activeElement;
      switch (action) {
        case "up":
        case "down":
        case "left":
        case "right":
          moveFocus(action);
          return;
        case "confirm":
          if (active instanceof HTMLElement && active !== document.body) {
            if (!typing(active)) active.click();
          } else {
            moveFocus("down");
          }
          return;
        case "back": {
          const dialog = [...document.querySelectorAll<HTMLDialogElement>("dialog[open]")].at(-1);
          if (dialog) {
            dialog.dispatchEvent(new Event("cancel", { cancelable: true }));
            return;
          }
          if (!tv || location.pathname !== "/tv") void navigate(-1);
          return;
        }
        case "home":
          void navigate("/tv");
          return;
        case "tertiary":
          if (tv) void navigate("/tv/search");
          else window.dispatchEvent(new CustomEvent(PAD_EVENT, { detail: action }));
          return;
        case "menu":
          if (tv) void navigate("/tv/settings");
          return;
        default:
          window.dispatchEvent(new CustomEvent(PAD_EVENT, { detail: action }));
      }
    });
    return () => {
      stop();
      window.removeEventListener("pointermove", usingMouse);
    };
  }, [navigate, location.pathname, tv]);
}
