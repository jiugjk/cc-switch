import { act, render, screen, waitFor } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { UpdateProvider, useUpdate } from "@/contexts/UpdateContext";

// F-001 回归：本 fork 已禁用应用内自动更新。UpdateProvider 不得在启动后自动
// 触发更新检查（上游行为：挂载 1 秒后 checkUpdate），hasUpdate 恒为 false。
const checkForUpdateMock = vi.fn(async () => ({ status: "up-to-date" as const }));

vi.mock("@/lib/updater", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/updater")>();
  return {
    ...actual,
    checkForUpdate: (...args: Parameters<typeof actual.checkForUpdate>) =>
      checkForUpdateMock(...(args as [])),
  };
});

function Probe() {
  const { hasUpdate, isChecking } = useUpdate();
  return (
    <div>
      <span data-testid="has-update">{String(hasUpdate)}</span>
      <span data-testid="is-checking">{String(isChecking)}</span>
    </div>
  );
}

describe("UpdateContext (fork: auto-update disabled)", () => {
  it("does not auto-check for updates after startup delay", () => {
    vi.useFakeTimers();
    render(
      <UpdateProvider>
        <Probe />
      </UpdateProvider>,
    );

    act(() => {
      vi.advanceTimersByTime(5000);
    });

    expect(checkForUpdateMock).not.toHaveBeenCalled();
    expect(screen.getByTestId("has-update")).toHaveTextContent("false");
    vi.useRealTimers();
  });

  it("manual checkUpdate resolves false and keeps hasUpdate false", async () => {
    let ctx: ReturnType<typeof useUpdate> | null = null;
    function Capture() {
      ctx = useUpdate();
      return null;
    }
    render(
      <UpdateProvider>
        <Capture />
      </UpdateProvider>,
    );

    let result: boolean | undefined;
    await act(async () => {
      result = await ctx!.checkUpdate();
    });

    expect(result).toBe(false);
    await waitFor(() => {
      expect(ctx!.hasUpdate).toBe(false);
    });
  });
});
