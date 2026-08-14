import type { ReactNode } from "react";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { act } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  INSTALLED_APPS_QUERY_KEY,
  mergeShownApps,
  probeInstalledApps,
  useInstalledApps,
} from "@/hooks/useInstalledApps";
import type { VisibleApps } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

type FocusHandler = (event: { payload: boolean }) => void;
const focusHandlers: FocusHandler[] = [];
const unlistenMock = vi.fn();

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onFocusChanged: async (handler: FocusHandler) => {
      focusHandlers.push(handler);
      return unlistenMock;
    },
  }),
}));

const emitWindowFocus = (focused: boolean) => {
  focusHandlers.forEach((handler) => handler({ payload: focused }));
};

const toolVersion = (name: string, version: string | null, error?: string) => ({
  name,
  version,
  latest_version: null,
  error: error ?? null,
  installed_but_broken: false,
  env_type: "windows",
  wsl_distro: null,
});

const allVisible: VisibleApps = {
  claude: true,
  "claude-desktop": true,
  codex: true,
  gemini: true,
  grokbuild: true,
  opencode: true,
  openclaw: true,
  hermes: true,
  pi: true,
};

interface WrapperProps {
  children: ReactNode;
}

function createWrapper() {
  const queryClient = createTestQueryClient();
  const wrapper = ({ children }: WrapperProps) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { wrapper, queryClient };
}

const countProbeCalls = () =>
  invokeMock.mock.calls.filter(([cmd]) => cmd === "get_tool_versions").length;

describe("mergeShownApps", () => {
  it("returns user visibility untouched before probe results arrive", () => {
    const prefs = { ...allVisible, gemini: false };
    expect(mergeShownApps(prefs, null)).toEqual(prefs);
  });

  it("hides apps detected as not installed", () => {
    const shown = mergeShownApps(allVisible, { codex: false, claude: true });
    expect(shown.codex).toBe(false);
    expect(shown.claude).toBe(true);
  });

  it("keeps unknown apps visible (fail-open)", () => {
    const shown = mergeShownApps(allVisible, { codex: false });
    expect(shown.opencode).toBe(true);
    expect(shown.hermes).toBe(true);
  });

  it("never overrides a user-hidden app even when installed", () => {
    const prefs = { ...allVisible, claude: false };
    const shown = mergeShownApps(prefs, { claude: true });
    expect(shown.claude).toBe(false);
  });
});

describe("probeInstalledApps", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  const mockCodexProbe = (options: {
    cliVersion: string | null;
    desktop: boolean | Error;
  }) => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_tool_versions") {
        return Promise.resolve([toolVersion("codex", options.cliVersion)]);
      }
      if (command === "is_claude_desktop_installed") {
        return Promise.resolve(true);
      }
      if (command === "is_codex_desktop_installed") {
        return options.desktop instanceof Error
          ? Promise.reject(options.desktop)
          : Promise.resolve(options.desktop);
      }
      return Promise.resolve(null);
    });
  };

  it("maps tool versions to installed flags with per-tool isolation", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_tool_versions") {
        return Promise.resolve([
          toolVersion("claude", "1.0.0"),
          toolVersion("codex", null, "not installed or not executable"),
          toolVersion("gemini", "2.0.0"),
          toolVersion("grok", "0.1.0"),
        ]);
      }
      if (command === "is_claude_desktop_installed") {
        return Promise.resolve(false);
      }
      if (command === "is_codex_desktop_installed") {
        return Promise.resolve(false);
      }
      return Promise.resolve(null);
    });

    const result = await probeInstalledApps();
    expect(result.claude).toBe(true);
    expect(result.codex).toBe(false);
    expect(result.gemini).toBe(true);
    expect(result.grokbuild).toBe(true);
    expect(result).not.toHaveProperty("grok");
    expect(result["claude-desktop"]).toBe(false);
  });

  it("treats Claude Desktop probe failure as installed (fail-open)", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_tool_versions") {
        return Promise.resolve([toolVersion("claude", "1.0.0")]);
      }
      if (command === "is_claude_desktop_installed") {
        return Promise.reject(new Error("probe failed"));
      }
      if (command === "is_codex_desktop_installed") {
        return Promise.resolve(false);
      }
      return Promise.resolve(null);
    });

    const result = await probeInstalledApps();
    expect(result["claude-desktop"]).toBe(true);
  });

  it("shows codex when only the CLI is installed", async () => {
    mockCodexProbe({ cliVersion: "0.44.0", desktop: false });
    const result = await probeInstalledApps();
    expect(result.codex).toBe(true);
  });

  it("shows codex when only the MS Store desktop app is installed", async () => {
    mockCodexProbe({ cliVersion: null, desktop: true });
    const result = await probeInstalledApps();
    expect(result.codex).toBe(true);
  });

  it("shows codex when both Codex clients are installed", async () => {
    mockCodexProbe({ cliVersion: "0.44.0", desktop: true });
    const result = await probeInstalledApps();
    expect(result.codex).toBe(true);
  });

  it("hides codex when neither Codex client is installed", async () => {
    mockCodexProbe({ cliVersion: null, desktop: false });
    const result = await probeInstalledApps();
    expect(result.codex).toBe(false);
  });

  it("falls back to the CLI result when the desktop probe fails (AppX query error)", async () => {
    mockCodexProbe({
      cliVersion: "0.44.0",
      desktop: new Error("appx query denied"),
    });
    const withCli = await probeInstalledApps();
    expect(withCli.codex).toBe(true);

    mockCodexProbe({
      cliVersion: null,
      desktop: new Error("appx query denied"),
    });
    const withoutCli = await probeInstalledApps();
    expect(withoutCli.codex).toBe(false);
  });
});

describe("useInstalledApps", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    unlistenMock.mockReset();
    focusHandlers.length = 0;
  });

  const mockProbe = (codexVersion: string | null) => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_tool_versions") {
        return Promise.resolve([
          toolVersion("claude", "1.0.0"),
          toolVersion("codex", codexVersion),
        ]);
      }
      if (command === "is_claude_desktop_installed") {
        return Promise.resolve(true);
      }
      if (command === "is_codex_desktop_installed") {
        return Promise.resolve(false);
      }
      return Promise.resolve(null);
    });
  };

  it("probes once on mount and exposes the result", async () => {
    mockProbe(null);
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useInstalledApps(), { wrapper });

    expect(result.current).toBeNull();
    await waitFor(() => {
      expect(result.current).not.toBeNull();
    });
    expect(result.current?.codex).toBe(false);
    expect(result.current?.claude).toBe(true);
    expect(countProbeCalls()).toBe(1);
  });

  it("re-probes on window focus once data is stale and reveals newly installed apps", async () => {
    mockProbe(null);
    const { wrapper, queryClient } = createWrapper();
    const { result } = renderHook(() => useInstalledApps(), { wrapper });
    await waitFor(() => {
      expect(result.current?.codex).toBe(false);
    });

    // 运行期间安装了 codex：下一次探测应返回已安装
    mockProbe("0.4.0");
    // 数据过期后窗口重新聚焦（invalidate 仅标记 stale，不主动 refetch）
    await queryClient.invalidateQueries({
      queryKey: INSTALLED_APPS_QUERY_KEY,
      refetchType: "none",
    });
    act(() => {
      emitWindowFocus(true);
    });

    await waitFor(() => {
      expect(result.current?.codex).toBe(true);
    });
    expect(countProbeCalls()).toBe(2);
  });

  it("throttles repeated focus events while data is fresh", async () => {
    mockProbe("0.4.0");
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useInstalledApps(), { wrapper });
    await waitFor(() => {
      expect(result.current).not.toBeNull();
    });

    act(() => {
      emitWindowFocus(true);
      emitWindowFocus(true);
      emitWindowFocus(true);
    });

    // 数据仍新鲜（staleTime 内）：反复聚焦不应重复探测
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(countProbeCalls()).toBe(1);
  });

  it("ignores blur events", async () => {
    mockProbe("0.4.0");
    const { wrapper, queryClient } = createWrapper();
    const { result } = renderHook(() => useInstalledApps(), { wrapper });
    await waitFor(() => {
      expect(result.current).not.toBeNull();
    });

    await queryClient.invalidateQueries({
      queryKey: INSTALLED_APPS_QUERY_KEY,
      refetchType: "none",
    });
    act(() => {
      emitWindowFocus(false);
    });

    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(countProbeCalls()).toBe(1);
  });

  it("keeps previous results when a re-probe fails entirely", async () => {
    mockProbe("0.4.0");
    const { wrapper, queryClient } = createWrapper();
    const { result } = renderHook(() => useInstalledApps(), { wrapper });
    await waitFor(() => {
      expect(result.current?.codex).toBe(true);
    });

    invokeMock.mockImplementation(() =>
      Promise.reject(new Error("probe blew up")),
    );
    await queryClient.invalidateQueries({
      queryKey: INSTALLED_APPS_QUERY_KEY,
      refetchType: "none",
    });
    act(() => {
      emitWindowFocus(true);
    });

    await waitFor(() => {
      expect(countProbeCalls()).toBe(2);
    });
    // 整次探测失败：保留上次结果，不清空按钮状态
    await waitFor(() => {
      expect(result.current?.codex).toBe(true);
      expect(result.current?.claude).toBe(true);
    });
  });

  it("stops listening after unmount", async () => {
    mockProbe("0.4.0");
    const { wrapper } = createWrapper();
    const { result, unmount } = renderHook(() => useInstalledApps(), {
      wrapper,
    });
    await waitFor(() => {
      expect(result.current).not.toBeNull();
    });

    unmount();
    expect(unlistenMock).toHaveBeenCalled();
  });
});
