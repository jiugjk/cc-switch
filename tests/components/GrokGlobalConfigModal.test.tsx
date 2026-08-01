import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GrokGlobalConfigModal } from "@/components/providers/forms/GrokGlobalConfigModal";

const apiMocks = vi.hoisted(() => ({
  read: vi.fn(),
  write: vi.fn(),
  preview: vi.fn(),
  listBackups: vi.fn(),
  restore: vi.fn(),
  deleteBackup: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  configApi: {
    readGrokGlobalConfig: apiMocks.read,
    writeGrokGlobalConfig: apiMocks.write,
    previewGrokPrivacyProtection: apiMocks.preview,
    listGrokConfigBackups: apiMocks.listBackups,
    restoreGrokConfigBackup: apiMocks.restore,
    deleteGrokConfigBackup: apiMocks.deleteBackup,
  },
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      aria-label="grok-config"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe("GrokGlobalConfigModal", () => {
  beforeEach(() => {
    Object.values(apiMocks).forEach((mock) => mock.mockReset());
    apiMocks.listBackups.mockResolvedValue([]);
    apiMocks.write.mockResolvedValue("");
  });

  it("disables privacy preview until the complete live config has loaded", async () => {
    const load = deferred<{
      path: string;
      directory: string;
      source: string;
      exists: boolean;
      content: string;
    }>();
    apiMocks.read.mockReturnValue(load.promise);

    render(<GrokGlobalConfigModal open onOpenChange={vi.fn()} />);

    const privacyButton = screen.getByRole("button", {
      name: /privacy settings|grokBuild\.privacyDraft/i,
    });
    expect(privacyButton).toBeDisabled();
    expect(apiMocks.preview).not.toHaveBeenCalled();

    load.resolve({
      path: "C:/Users/test/.grok/config.toml",
      directory: "C:/Users/test/.grok",
      source: "default",
      exists: true,
      content: '[models]\ndefault = "real"\n',
    });

    await waitFor(() => expect(privacyButton).toBeEnabled());
    expect(screen.getByLabelText("grok-config")).toHaveValue(
      '[models]\ndefault = "real"\n',
    );
  });

  it("ignores a privacy preview that finishes after a newer edit", async () => {
    const preview = deferred<string>();
    const onOpenChange = vi.fn();
    apiMocks.read.mockResolvedValue({
      path: "C:/Users/test/.grok/config.toml",
      directory: "C:/Users/test/.grok",
      source: "default",
      exists: true,
      content: '[models]\ndefault = "real"\n',
    });
    apiMocks.preview.mockReturnValue(preview.promise);

    render(<GrokGlobalConfigModal open onOpenChange={onOpenChange} />);

    const privacyButton = await screen.findByRole("button", {
      name: /privacy settings|grokBuild\.privacyDraft/i,
    });
    await waitFor(() => expect(privacyButton).toBeEnabled());
    fireEvent.click(privacyButton);
    expect(apiMocks.preview).toHaveBeenCalledWith(
      '[models]\ndefault = "real"\n',
    );
    const cancelButton = screen.getByRole("button", {
      name: /common\.cancel|cancel/i,
    });
    expect(cancelButton).toBeDisabled();
    fireEvent.click(cancelButton);
    expect(onOpenChange).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText("grok-config"), {
      target: { value: "# newer manual edit\n" },
    });
    preview.resolve("[features]\ntelemetry = false\n");

    await waitFor(() =>
      expect(screen.getByLabelText("grok-config")).toHaveValue(
        "# newer manual edit\n",
      ),
    );
  });

  it("keeps a newer draft when a deferred save returns", async () => {
    const saveRequest = deferred<string>();
    const onOpenChange = vi.fn();
    apiMocks.read.mockResolvedValue({
      path: "C:/Users/test/.grok/config.toml",
      directory: "C:/Users/test/.grok",
      source: "default",
      exists: true,
      content: "[features]\ntelemetry = true\n",
    });
    apiMocks.listBackups.mockResolvedValue([
      {
        filename: "grok-config-test.toml",
        path: "C:/Users/test/grok-config-test.toml",
        createdAt: "2026-08-01T00:00:00Z",
        sizeBytes: 32,
      },
    ]);
    apiMocks.write.mockReturnValue(saveRequest.promise);

    render(<GrokGlobalConfigModal open onOpenChange={onOpenChange} />);

    const editor = await screen.findByLabelText("grok-config");
    await waitFor(() =>
      expect(editor).toHaveValue("[features]\ntelemetry = true\n"),
    );
    const savedByBackend = "[features]\ntelemetry = false\n";
    fireEvent.change(editor, { target: { value: savedByBackend } });
    const saveButton = screen.getByRole("button", {
      name: /common\.save|save/i,
    });
    fireEvent.click(saveButton);

    await waitFor(() => expect(saveButton).toBeDisabled());
    expect(
      screen.getByRole("button", { name: /grokBuild\.restore|restore/i }),
    ).toBeDisabled();
    const cancelButton = screen.getByRole("button", {
      name: /common\.cancel|cancel/i,
    });
    expect(cancelButton).toBeDisabled();
    fireEvent.click(cancelButton);
    expect(onOpenChange).not.toHaveBeenCalled();

    const newerDraft =
      "[features]\ntelemetry = false\n\n[harness]\nfuture_flag = true\n";
    fireEvent.change(editor, { target: { value: newerDraft } });
    saveRequest.resolve(savedByBackend);

    await waitFor(() => expect(saveButton).toBeEnabled());
    expect(editor).toHaveValue(newerDraft);

    // The backend response still becomes the saved baseline. Returning the
    // editor to that exact content must clear dirty state without another save.
    fireEvent.change(editor, { target: { value: savedByBackend } });
    expect(saveButton).toBeDisabled();
  });

  it("keeps the loaded config usable when backup enumeration fails", async () => {
    apiMocks.read.mockResolvedValue({
      path: "C:/Users/test/.grok/config.toml",
      directory: "C:/Users/test/.grok",
      source: "default",
      exists: true,
      content: "[features]\ntelemetry = true\n",
    });
    apiMocks.listBackups.mockRejectedValue(new Error("backup directory busy"));

    render(<GrokGlobalConfigModal open onOpenChange={vi.fn()} />);

    await waitFor(() =>
      expect(screen.getByLabelText("grok-config")).toHaveValue(
        "[features]\ntelemetry = true\n",
      ),
    );
    expect(
      screen.getByRole("button", {
        name: /privacy settings|grokBuild\.privacyDraft/i,
      }),
    ).toBeEnabled();
  });
});
