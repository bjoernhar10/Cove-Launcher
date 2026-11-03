import type { AchievementProgress } from "@cove/shared";
import { ProgressBar } from "../ui/Feedback.tsx";

/** "12/40" + thin bar. Renders a dash when a game has no achievements. */
export function AchievementMeter({ progress, className }: { progress: AchievementProgress; className?: string }) {
  if (progress.total === 0) {
    return <span className="text-xs text-fg-subtle">Keine Achievements</span>;
  }
  return (
    <div className={className}>
      <div className="flex items-center justify-between text-xs text-fg-muted">
        <span>Achievements</span>
        <span className="num">
          {progress.unlocked}/{progress.total}
        </span>
      </div>
      <ProgressBar
        value={progress.unlocked}
        max={progress.total}
        label={`${String(progress.unlocked)} von ${String(progress.total)} Achievements`}
        className="mt-1.5"
      />
    </div>
  );
}
