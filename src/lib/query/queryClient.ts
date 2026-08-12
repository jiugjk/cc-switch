import { QueryClient } from "@tanstack/react-query";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: true,
      // Most data is invalidated explicitly after mutations. A short default
      // cache window avoids a burst of duplicate invokes when users switch
      // tabs or remount a settings panel, while still picking up external CLI
      // edits without making every query opt in individually.
      staleTime: 30_000,
    },
    mutations: {
      retry: false,
    },
  },
});
