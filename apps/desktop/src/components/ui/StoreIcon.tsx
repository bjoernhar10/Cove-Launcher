import type { ConnectableStore } from "@cove/shared";
import { Package } from "lucide-react";
import { siEpicgames, siGogdotcom, siItchdotio, siSteam } from "simple-icons";

/** Single-path brand marks (Simple Icons, CC0), drawn in the current text colour. */
const PATHS: Record<Exclude<ConnectableStore, "amazon"> | "itch", string> = {
  steam: siSteam.path,
  gog: siGogdotcom.path,
  epic: siEpicgames.path,
  itch: siItchdotio.path,
};

/**
 * A store's logo as a calm monochrome mark: same 18px size as every other icon,
 * no brand colours. Decorative, the store name always stands next to it.
 */
export function StoreIcon({ store }: { store: ConnectableStore | "itch" }) {
  // Simple Icons has no Amazon mark (trademark), so Amazon gets a neutral package.
  if (store === "amazon") return <Package aria-hidden />;
  return (
    <svg aria-hidden viewBox="0 0 24 24" width={18} height={18} fill="currentColor" focusable="false">
      <path d={PATHS[store]} />
    </svg>
  );
}
