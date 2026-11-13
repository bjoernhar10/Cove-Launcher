import type { GamepadInfo, GamepadInput } from "@cove/shared";
import { call, subscribe } from "./transport.ts";

/** Connected controllers. */
export function list(): Promise<GamepadInfo[]> {
  return call("gamepad_list", {});
}

/** Controller actions; the core sends none while a game runs or Cove isn't focused. */
export function subscribeInput(handler: (input: GamepadInput) => void): () => void {
  return subscribe("gamepad-input", handler);
}

export function subscribeChanged(handler: (pads: GamepadInfo[]) => void): () => void {
  return subscribe("gamepads-changed", handler);
}

/** Steam Deck gaming mode or a gamescope session: start in the TV mode. */
export function tvHint(): Promise<boolean> {
  return call("system_tv_hint", {});
}

export async function setFullscreen(fullscreen: boolean): Promise<void> {
  await call("window_set_fullscreen", { fullscreen });
}
