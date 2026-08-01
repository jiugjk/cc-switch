import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AboutSection } from "@/components/settings/AboutSection";

// F-001 回归：本发行版已禁用应用内自动更新。「检查更新」按钮必须走
// check_for_updates（打开发行版发布页）的手动流程，绝不触发
// install_update_and_restart（上游一键下载安装路径）。

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, _opts?: unknown) => key,
  }),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "3.17.0"),
}));

const checkUpdatesMock = vi.fn(async () => undefined);
const installUpdateAndRestartMock = vi.fn(async () => true);
const getToolVersionsMock = vi.fn(async () => []);
const probeToolInstallationsMock = vi.fn(async () => []);
const openExternalMock = vi.fn(async () => undefined);

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    settingsApi: {
      ...actual.settingsApi,
      checkUpdates: (...args: unknown[]) => checkUpdatesMock(...(args as [])),
      installUpdateAndRestart: (...args: unknown[]) =>
        installUpdateAndRestartMock(...(args as [])),
      getToolVersions: (...args: unknown[]) =>
        getToolVersionsMock(...(args as [])),
      probeToolInstallations: (...args: unknown[]) =>
        probeToolInstallationsMock(...(args as [])),
      openExternal: (...args: unknown[]) => openExternalMock(...(args as [])),
    },
  };
});

function renderAboutSection() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <AboutSection isPortable={false} />
    </QueryClientProvider>,
  );
}

describe("AboutSection check-for-updates (distribution: manual flow only)", () => {
  beforeEach(() => {
    checkUpdatesMock.mockClear();
    installUpdateAndRestartMock.mockClear();
  });

  it("opens the releases page and never calls install_update_and_restart", async () => {
    renderAboutSection();

    const button = await screen.findByRole("button", {
      name: /settings\.checkForUpdates/,
    });
    fireEvent.click(button);

    await waitFor(() => {
      expect(checkUpdatesMock).toHaveBeenCalledTimes(1);
    });
    expect(installUpdateAndRestartMock).not.toHaveBeenCalled();
  });
});
