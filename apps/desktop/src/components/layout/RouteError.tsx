import { useMessages } from "../../i18n/index.ts";
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
  const t = useMessages();
  const error = useRouteError();
  const notFound = isRouteErrorResponse(error) && error.status === 404;

  useEffect(() => {
    if (!notFound) console.error("[cove] page crashed:", error);
  }, [error, notFound]);

  return (
    <Page>
      <EmptyState
        icon={CircleAlert}
        title={notFound ? t.routeError.notFoundTitle : t.routeError.crashTitle}
        description={
          notFound
            ? t.routeError.notFoundText
            : t.routeError.crashText
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
                {t.routeError.reload}
              </Button>
            )}
            <ButtonLink to="/">{t.routeError.home}</ButtonLink>
          </>
        }
      />
    </Page>
  );
}
