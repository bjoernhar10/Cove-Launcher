import { CircleAlert } from "lucide-react";
import { useEffect } from "react";
import { isRouteErrorResponse, useRouteError } from "react-router";
import { Button, ButtonLink } from "../ui/Button.tsx";
import { EmptyState } from "../ui/Feedback.tsx";
import { Page } from "../ui/Page.tsx";

/**
 * Shown instead of React Router's developer screen when a page crashes.
 * The technical details go to the console, never into the UI.
 */
export function RouteError() {
  const error = useRouteError();
  const notFound = isRouteErrorResponse(error) && error.status === 404;

  useEffect(() => {
    if (!notFound) console.error("[cove] Seite abgestürzt:", error);
  }, [error, notFound]);

  return (
    <Page>
      <EmptyState
        icon={CircleAlert}
        title={notFound ? "Diese Seite gibt es nicht" : "Hier ist etwas schiefgelaufen"}
        description={
          notFound
            ? "Vielleicht wurde sie verschoben. Zurück zur Startseite?"
            : "Diese Ansicht konnte nicht angezeigt werden. Deine Daten sind sicher. Lade die Ansicht neu oder geh zurück zur Startseite."
        }
        actions={
          <>
            {!notFound && (
              <Button
                variant="primary"
                onClick={() => {
                  window.location.reload();
                }}
              >
                Neu laden
              </Button>
            )}
            <ButtonLink to="/">Zu Home</ButtonLink>
          </>
        }
      />
    </Page>
  );
}
