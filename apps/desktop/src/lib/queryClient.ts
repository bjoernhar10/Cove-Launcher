import { QueryClient } from "@tanstack/react-query";

/** Shared TanStack Query client (catalogue data; cached in memory for 5 minutes). */
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5 * 60 * 1000,
      gcTime: 30 * 60 * 1000,
      retry: false,
      refetchOnWindowFocus: false,
    },
  },
});
