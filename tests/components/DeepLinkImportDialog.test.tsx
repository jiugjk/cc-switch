import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
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

    const preview = screen.getByTestId("deeplink-generic-config-preview");
    expect(preview.textContent).toContain(`sk-e${"*".repeat(12)}`);
    expect(preview.textContent).not.toContain("sk-embedded-secret-key");
    expect(preview.textContent).toContain("baseUrl");
    expect(preview.textContent).toContain("https://hermes.example.com");

    // Main API key uses the shared deeplink masking policy.
    expect(screen.getByText(`sk-h${"*".repeat(12)}`)).toBeInTheDocument();
    expect(screen.queryByText("sk-hermes-full-key-123456")).toBeNull();

    // Short usage API key is fully masked, never shown verbatim
    expect(screen.getByText("****")).toBeInTheDocument();
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
    expect(screen.getByText("****")).toBeInTheDocument();
    expect(screen.queryByText("tiny")).toBeNull();
    // No config preview section without config
    expect(screen.queryByText("deeplink.configDetails")).toBeNull();
  });

  it("preserves arrival order and cancel advances to the next queued event", async () => {
    let releaseFirstMerge!: () => void;
    const firstMergeGate = new Promise<void>((resolve) => {
      releaseFirstMerge = resolve;
    });
    server.use(
      http.post(
        "http://tauri.local/merge_deeplink_config",
        async ({ request }) => {
          const body = (await request.json()) as {
            request: Record<string, unknown>;
          };
          await firstMergeGate;
          return HttpResponse.json(body.request);
        },
      ),
    );

    renderDialog();
    await act(async () => {});
    await act(async () => {
      emitTauriEvent("deeplink-import", {
        version: "1",
        resource: "provider",
        app: "hermes",
        name: "First Slow Link",
        apiKey: "first-key",
        config: b64Json({ apiKey: "first-key" }),
      });
      emitTauriEvent("deeplink-import", {
        version: "1",
        resource: "provider",
        app: "hermes",
        name: "Second Fast Link",
        apiKey: "second-key",
      });
    });

    expect(screen.queryByText("Second Fast Link")).toBeNull();
    await act(async () => releaseFirstMerge());
    await screen.findByText("First Slow Link");
    expect(screen.queryByText("Second Fast Link")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    await screen.findByText("Second Fast Link");
    expect(screen.queryByText("First Slow Link")).toBeNull();
  });

  it("shows disabled MCP semantics unless enabled is explicitly true", async () => {
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
    await act(async () => {});
    const config = b64Json({
      mcpServers: { demo: { command: "echo", args: ["demo"] } },
    });

    await act(async () => {
      emitTauriEvent("deeplink-import", {
        version: "1",
        resource: "mcp",
        apps: "codex",
        config,
      });
    });
    await screen.findByText("deeplink.mcp.disabledNotice");
    expect(screen.queryByText("deeplink.mcp.enabledWarning")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    await act(async () => {
      emitTauriEvent("deeplink-import", {
        version: "1",
        resource: "mcp",
        apps: "codex",
        config,
        enabled: true,
      });
    });
    await screen.findByText("deeplink.mcp.enabledWarning");
    expect(screen.queryByText("deeplink.mcp.disabledNotice")).toBeNull();
  });

  it("renders masked usage access token and user id for provider imports", async () => {
    renderDialog();
    await act(async () => {});

    await act(async () => {
      emitTauriEvent("deeplink-import", {
        version: "v1",
        resource: "provider",
        app: "claude",
        name: "Test Provider",
        homepage: "https://example.com",
        endpoint: "https://api.example.com",
        apiKey: "sk-provider-key",
        usageEnabled: true,
        usageScript: btoa("console.log('usage');"),
        usageApiKey: "sk-usage-key",
        usageBaseUrl: "https://usage.example.com",
        usageAccessToken: "pat-secret-token",
        usageUserId: "user-12345",
        usageAutoInterval: 60,
      });
    });

    await screen.findByText("用量访问令牌");
    expect(screen.getByText("用量用户 ID")).toBeInTheDocument();
    expect(screen.getByText("user-12345")).toBeInTheDocument();
    expect(screen.getByText("pat-************")).toBeInTheDocument();
  });

  it("shows usage credentials even when the deeplink carries no usageScript", async () => {
    renderDialog();
    await act(async () => {});

    await act(async () => {
      emitTauriEvent("deeplink-import", {
        version: "v1",
        resource: "provider",
        app: "claude",
        name: "Token Only Provider",
        homepage: "https://example.com",
        endpoint: "https://api.example.com",
        apiKey: "sk-provider-key",
        usageAccessToken: "pat-secret-token",
        usageUserId: "user-12345",
      });
    });

    await screen.findByText("用量访问令牌");
    expect(screen.getByText("pat-************")).toBeInTheDocument();
    expect(screen.getByText("用量用户 ID")).toBeInTheDocument();
    expect(screen.getByText("user-12345")).toBeInTheDocument();
    expect(
      screen.queryByText(
        "这是一段 JavaScript 代码，启用后会在查询用量时执行。请确认来源可信后再导入。",
      ),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("脚本代码")).not.toBeInTheDocument();
  });
});
