import { MapPinOff } from "lucide-react";
import { ButtonLink } from "../components/ui/Button.tsx";
import { EmptyState } from "../components/ui/Feedback.tsx";
import { Page } from "../components/ui/Page.tsx";

export function NotFoundPage() {
  return (
    <Page>
      <EmptyState
        icon={MapPinOff}
        title="Diese Seite gibt es nicht"
        description="Vielleicht wurde sie verschoben. Zurück zur Startseite?"
        actions={<ButtonLink to="/">Zu Home</ButtonLink>}
      />
    </Page>
  );
}
