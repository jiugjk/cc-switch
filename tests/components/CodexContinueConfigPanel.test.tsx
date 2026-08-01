import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexContinueConfigPanel } from "@/components/settings/CodexContinueConfigPanel";

const apiMocks = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn(),
}));

vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    getCodexContinueConfig: apiMocks.get,
    setCodexContinueConfig: apiMocks.set,
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key,
  }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

const loadedConfig = {
  enabled: true,
  maxContinuations: 8,
  step: 518,
  marker: "continue",
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe("CodexContinueConfigPanel", () => {
  beforeEach(() => {
    apiMocks.get.mockReset().mockResolvedValue(loadedConfig);
    apiMocks.set.mockReset().mockResolvedValue(true);
  });

  it("keeps unsaved advanced edits when toggling enabled", async () => {
    render(<CodexContinueConfigPanel />);

    const maxInput = await screen.findByLabelText("最大续写轮数");
    const markerInput = screen.getByLabelText("续写提示");
    fireEvent.change(maxInput, { target: { value: "17" } });
    fireEvent.change(markerInput, { target: { value: "draft marker" } });

    fireEvent.click(screen.getByRole("switch"));

    await waitFor(() =>
      expect(apiMocks.set).toHaveBeenCalledWith({
        ...loadedConfig,
        enabled: false,
      }),
    );
    expect(maxInput).toHaveValue(17);
    expect(markerInput).toHaveValue("draft marker");
  });

  it("clamps advanced max continuations to 32 before saving", async () => {
    render(<CodexContinueConfigPanel />);

    const maxInput = await screen.findByLabelText("最大续写轮数");
    fireEvent.change(maxInput, { target: { value: "99" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(apiMocks.set).toHaveBeenCalledWith({
        ...loadedConfig,
        maxContinuations: 32,
      }),
    );
  });

  it("serializes rapid toggles in the same order as the latest user intent", async () => {
    const first = deferred<boolean>();
    const second = deferred<boolean>();
    apiMocks.set
      .mockReset()
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise);

    render(<CodexContinueConfigPanel />);

    const toggle = await screen.findByRole("switch");
    fireEvent.click(toggle);
    fireEvent.click(toggle);

    expect(toggle).toBeChecked();
    await waitFor(() => expect(apiMocks.set).toHaveBeenCalledTimes(1));
    expect(apiMocks.set).toHaveBeenNthCalledWith(1, {
      ...loadedConfig,
      enabled: false,
    });

    // The second command cannot overtake the first one at the backend.
    expect(apiMocks.set).toHaveBeenCalledTimes(1);
    first.resolve(true);
    await waitFor(() => expect(apiMocks.set).toHaveBeenCalledTimes(2));
    expect(apiMocks.set).toHaveBeenNthCalledWith(2, loadedConfig);

    second.resolve(true);
    await waitFor(() => expect(toggle).toBeChecked());
  });

  it("does not let an older failed save roll back a newer queued intent", async () => {
    const first = deferred<boolean>();
    const second = deferred<boolean>();
    apiMocks.set
      .mockReset()
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise);

    render(<CodexContinueConfigPanel />);

    const toggle = await screen.findByRole("switch");
    fireEvent.click(toggle);
    fireEvent.click(toggle);
    expect(toggle).toBeChecked();

    first.reject(new Error("older write failed"));
    await waitFor(() => expect(apiMocks.set).toHaveBeenCalledTimes(2));
    expect(toggle).toBeChecked();

    second.resolve(true);
    await waitFor(() => expect(toggle).toBeChecked());
  });

  it("rolls the latest failed intent back to the last committed value", async () => {
    const request = deferred<boolean>();
    apiMocks.set.mockReset().mockReturnValue(request.promise);

    render(<CodexContinueConfigPanel />);

    const toggle = await screen.findByRole("switch");
    fireEvent.click(toggle);
    expect(toggle).not.toBeChecked();

    request.reject(new Error("save failed"));
    await waitFor(() => expect(toggle).toBeChecked());
  });
});
