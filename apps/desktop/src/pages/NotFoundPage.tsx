import { useMessages } from "../i18n/index.ts";
import { MapPinOff } from "lucide-react";
import { ButtonLink } from "../components/ui/Button.tsx";
import { EmptyState } from "../components/ui/Feedback.tsx";
import { Page } from "../components/ui/Page.tsx";

export function NotFoundPage() {
  const t = useMessages();
  return (
    <Page>
      <EmptyState
        icon={MapPinOff}
        title={t.routeError.notFoundTitle}
        description={t.routeError.notFoundText}
        actions={<ButtonLink to="/">{t.routeError.home}</ButtonLink>}
      />
    </Page>
  );
}
