import { createHashRouter, Navigate } from "react-router";
import { AppShell } from "./components/layout/AppShell.tsx";
import { CatalogueDetailPage } from "./pages/CatalogueDetailPage.tsx";
import { CataloguePage } from "./pages/CataloguePage.tsx";
import { DownloadsPage } from "./pages/DownloadsPage.tsx";
import { HomePage } from "./pages/HomePage.tsx";
import { LibraryDetailPage } from "./pages/LibraryDetailPage.tsx";
import { LibraryPage } from "./pages/LibraryPage.tsx";
import { NotFoundPage } from "./pages/NotFoundPage.tsx";
import { SettingsPage } from "./pages/settings/SettingsPage.tsx";

export const routes = [
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <HomePage /> },
      { path: "catalogue", element: <CataloguePage /> },
      { path: "catalogue/:gameId", element: <CatalogueDetailPage /> },
      { path: "library", element: <LibraryPage /> },
      { path: "library/:gameId", element: <LibraryDetailPage /> },
      { path: "downloads", element: <DownloadsPage /> },
      { path: "settings", element: <Navigate to="/settings/general" replace /> },
      { path: "settings/:tab", element: <SettingsPage /> },
      { path: "*", element: <NotFoundPage /> },
    ],
  },
];

/** Hash routing: works identically in the browser and under Tauri's custom protocol. */
export function createAppRouter() {
  return createHashRouter(routes);
}
