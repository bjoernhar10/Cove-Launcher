import { useState } from "react";
import { NavigationType, useLocation, useNavigate, useNavigationType } from "react-router";

function currentIndex(): number {
  const state: unknown = window.history.state;
  if (typeof state === "object" && state !== null && "idx" in state && typeof state.idx === "number") {
    return state.idx;
  }
  return 0;
}

/** Back/forward availability for the topbar buttons. */
export function useHistoryNav(): { canBack: boolean; canForward: boolean; back: () => void; forward: () => void } {
  const location = useLocation();
  const navType = useNavigationType();
  const navigate = useNavigate();
  const index = currentIndex();
  const [track, setTrack] = useState({ key: location.key, max: index });

  // Adjust state while rendering when the location changes (no effect needed).
  if (track.key !== location.key) {
    // A push discards the forward history.
    setTrack({ key: location.key, max: navType === NavigationType.Push ? index : Math.max(track.max, index) });
  }

  return {
    canBack: index > 0,
    canForward: index < track.max,
    back: () => {
      void navigate(-1);
    },
    forward: () => {
      void navigate(1);
    },
  };
}
