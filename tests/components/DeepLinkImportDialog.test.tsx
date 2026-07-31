import { act, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { server } from "../msw/server";
import { emitTauriEvent } from "../msw/tauriMocks";

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({
    open,
    children,
  }: {
    open?: boolean;
    children: React.ReactNode;
  }) => (open ? <div>{children}</div> : null),
  DialogContent: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogHeader: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogTitle: ({ children }: { children: React.ReactNode }) => (
    <h1>{children}</h1>
  ),
  DialogDescription: ({ children }: { children: React.ReactNode }) => (
    <p>{children}</p>
  ),
  DialogFooter: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
}));

const b64Json = (value: unknown): string =>
  Buffer.from(JSON.stringify(value), "utf-8").toString("base64");

const renderDialog = () => {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <DeepLinkImportDialog />
    </QueryClientProvider>,
  );
};

describe("DeepLinkImportDialog", () => {
  it("renders a masked config preview for a hermes deep link with embedded config", async () => {
    // Echo the request back from merge_deeplink_config
    server.use(
      http.post(
        "http://tauri.local/merge_deeplink_config",
        async ({ request }) => {
          const body = (await request.json()) as {
            request: Record<string, unknown>;
          };
          return HttpResponse.json(body.request);
        },
      ),
    );

    renderDialog();
    // Flush the async listen() registration before emitting
    await act(async () => {});

    await act(async () => {
      emitTauriEvent("deeplink-import", {
        version: "1",
        resource: "provider",
        app: "hermes",
        name: "Hermes Provider",
        apiKey: "sk-hermes-full-key-123456",
        endpoint: "https://hermes.example.com",
        usageScript: "builtin",
        usageApiKey: "short1",
        config: b64Json({
          apiKey: "sk-embedded-secret-key",
          baseUrl: "https://hermes.example.com",
        }),
      });
    });

    // Config preview section appears (previously null for hermes)
    await screen.findByText("deeplink.configDetails");

    // Embedded sensitive entry is masked (4-char prefix + 8 stars + 2-char suffix)
    expect(screen.getByText(`sk-e${"*".repeat(8)}ey`)).toBeInTheDocument();
    expect(screen.queryByText("sk-embedded-secret-key")).toBeNull();

    // Non-sensitive entry is shown verbatim
    expect(screen.getByText("baseUrl")).toBeInTheDocument();

    // Main API key goes through maskSecret
    expect(screen.getByText(`sk-h${"*".repeat(8)}56`)).toBeInTheDocument();
    expect(screen.queryByText("sk-hermes-full-key-123456")).toBeNull();

    // Short usage API key is fully masked, never shown verbatim
    expect(screen.getByText("***")).toBeInTheDocument();
    expect(screen.queryByText("short1")).toBeNull();
  });

  it("shows the dialog for a hermes request without embedded config", async () => {
    renderDialog();
    await act(async () => {});

    await act(async () => {
      emitTauriEvent("deeplink-import", {
        version: "1",
        resource: "provider",
        app: "hermes",
        name: "Plain Hermes",
        apiKey: "tiny",
      });
    });

    await waitFor(() =>
      expect(screen.getByText("Plain Hermes")).toBeInTheDocument(),
    );
    // Short main API key is fully masked (unified helper), not shown verbatim
    expect(screen.getByText("***")).toBeInTheDocument();
    expect(screen.queryByText("tiny")).toBeNull();
    // No config preview section without config
    expect(screen.queryByText("deeplink.configDetails")).toBeNull();
  });
});
