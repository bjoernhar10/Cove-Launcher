import { createHashRouter, Navigate } from "react-router";
import { AppShell } from "./components/layout/AppShell.tsx";
import { RouteError } from "./components/layout/RouteError.tsx";
import { HomePage } from "./pages/HomePage.tsx";
import { LibraryPage } from "./pages/LibraryPage.tsx";
import { NotFoundPage } from "./pages/NotFoundPage.tsx";

// Home and Library are where Cove opens; everything else loads on first visit
// so the start bundle stays small.
export const routes = [
  {
    path: "/",
    element: <AppShell />,
    errorElement: <RouteError />,
    children: [
      {
        errorElement: <RouteError />,
        children: [
          { index: true, element: <HomePage /> },
          { path: "library", element: <LibraryPage /> },
          {
            path: "catalogue",
            lazy: async () => ({ Component: (await import("./pages/CataloguePage.tsx")).CataloguePage }),
          },
          {
            path: "catalogue/:gameId",
            lazy: async () => ({ Component: (await import("./pages/CatalogueDetailPage.tsx")).CatalogueDetailPage }),
          },
          {
            path: "library/:gameId",
            lazy: async () => ({ Component: (await import("./pages/LibraryDetailPage.tsx")).LibraryDetailPage }),
          },
          {
            path: "downloads",
            lazy: async () => ({ Component: (await import("./pages/DownloadsPage.tsx")).DownloadsPage }),
          },
          {
            path: "wishlist",
            lazy: async () => ({ Component: (await import("./pages/WishlistPage.tsx")).WishlistPage }),
          },
          {
            path: "stats",
            lazy: async () => ({ Component: (await import("./pages/StatsPage.tsx")).StatsPage }),
          },
          { path: "settings", element: <Navigate to="/settings/general" replace /> },
          {
            path: "settings/:tab",
            lazy: async () => ({ Component: (await import("./pages/settings/SettingsPage.tsx")).SettingsPage }),
          },
          { path: "*", element: <NotFoundPage /> },
        ],
      },
    ],
  },
];

/** Hash routing: works identically in the browser and under Tauri's custom protocol. */
export function createAppRouter() {
  return createHashRouter(routes);
}
