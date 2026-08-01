import { useCallback, useEffect, useRef, useState } from "react";
import { Loader2, RotateCcw, ShieldCheck, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import JsonEditor from "@/components/JsonEditor";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useDarkMode } from "@/hooks/useDarkMode";
import { configApi } from "@/lib/api";
import type { GrokConfigBackup } from "@/lib/api/config";

interface GrokGlobalConfigModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function GrokGlobalConfigModal({
  open,
  onOpenChange,
}: GrokGlobalConfigModalProps) {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();
  const [content, setContent] = useState("");
  const [savedContent, setSavedContent] = useState("");
  const [path, setPath] = useState("");
  const [source, setSource] = useState("");
  const [backups, setBackups] = useState<GrokConfigBackup[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [hasLoadedConfig, setHasLoadedConfig] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isPreviewing, setIsPreviewing] = useState(false);
  const [isRestoring, setIsRestoring] = useState(false);
  const documentVersionRef = useRef(0);
  const previewRequestRef = useRef(0);
  const isDirty = content !== savedContent;
  const isBusy = isSaving || isPreviewing || isRestoring;

  const loadBackups = useCallback(async () => {
    setBackups(await configApi.listGrokConfigBackups());
  }, []);

  useEffect(() => {
    if (!open) return;
    let active = true;
    documentVersionRef.current += 1;
    previewRequestRef.current += 1;
    setIsPreviewing(false);
    setIsLoading(true);
    setHasLoadedConfig(false);
    void configApi
      .readGrokGlobalConfig()
      .then((config) => {
        if (!active) return;
        documentVersionRef.current += 1;
        previewRequestRef.current += 1;
        setIsPreviewing(false);
        setContent(config.content);
        setSavedContent(config.content);
        setPath(config.path);
        setSource(config.source);
        setHasLoadedConfig(true);
      })
      .catch((error) => toast.error(errorText(error)))
      .finally(() => active && setIsLoading(false));
    void configApi
      .listGrokConfigBackups()
      .then((nextBackups) => active && setBackups(nextBackups))
      .catch((error) => toast.error(errorText(error)));
    return () => {
      active = false;
      previewRequestRef.current += 1;
    };
  }, [open]);

  const updateDraft = (nextContent: string) => {
    documentVersionRef.current += 1;
    previewRequestRef.current += 1;
    setIsPreviewing(false);
    setContent(nextContent);
  };

  const requestOpenChange = (nextOpen: boolean) => {
    if (!nextOpen && isBusy) {
      return;
    }
    if (
      !nextOpen &&
      isDirty &&
      !window.confirm(
        t("grokBuild.globalDiscardConfirm", {
          defaultValue: "Discard unsaved config.toml changes?",
        }),
      )
    ) {
      return;
    }
    onOpenChange(nextOpen);
  };

  const save = async () => {
    const documentVersion = documentVersionRef.current;
    setIsSaving(true);
    try {
      const saved = await configApi.writeGrokGlobalConfig(content);
      if (documentVersion === documentVersionRef.current) {
        documentVersionRef.current += 1;
        previewRequestRef.current += 1;
        setContent(saved);
      }
      setSavedContent(saved);
      await loadBackups();
      toast.success(
        t("grokBuild.globalSaveSuccess", {
          defaultValue: "Grok Build global configuration saved",
        }),
      );
    } catch (error) {
      toast.error(errorText(error));
    } finally {
      setIsSaving(false);
    }
  };

  const applyPrivacyDraft = async () => {
    const requestId = ++previewRequestRef.current;
    const documentVersion = documentVersionRef.current;
    const draft = content;
    setIsPreviewing(true);
    try {
      const preview = await configApi.previewGrokPrivacyProtection(draft);
      if (
        requestId !== previewRequestRef.current ||
        documentVersion !== documentVersionRef.current
      ) {
        return;
      }
      documentVersionRef.current += 1;
      setContent(preview);
      toast.info(
        t("grokBuild.privacyDrafted", {
          defaultValue:
            "Privacy settings were added to the draft. Review the exact TOML, then save to apply.",
        }),
      );
    } catch (error) {
      if (requestId === previewRequestRef.current) {
        toast.error(errorText(error));
      }
    } finally {
      if (requestId === previewRequestRef.current) {
        setIsPreviewing(false);
      }
    }
  };

  const restoreBackup = async (backup: GrokConfigBackup) => {
    if (
      !window.confirm(
        t("grokBuild.restoreConfirm", {
          defaultValue:
            "Restore backup {{filename}}? The current file will be backed up first.",
          filename: backup.filename,
        }),
      )
    ) {
      return;
    }
    const documentVersion = documentVersionRef.current;
    setIsRestoring(true);
    try {
      const restored = await configApi.restoreGrokConfigBackup(backup.filename);
      if (documentVersion === documentVersionRef.current) {
        documentVersionRef.current += 1;
        previewRequestRef.current += 1;
        setContent(restored);
      }
      setSavedContent(restored);
      await loadBackups();
      toast.success(
        t("grokBuild.backupRestored", { defaultValue: "Backup restored" }),
      );
    } catch (error) {
      toast.error(errorText(error));
    } finally {
      setIsRestoring(false);
    }
  };

  const deleteBackup = async (backup: GrokConfigBackup) => {
    if (
      !window.confirm(
        t("grokBuild.deleteBackupConfirm", {
          defaultValue: "Permanently delete backup {{filename}}?",
          filename: backup.filename,
        }),
      )
    ) {
      return;
    }
    try {
      await configApi.deleteGrokConfigBackup(backup.filename);
      await loadBackups();
      toast.success(
        t("grokBuild.backupDeleted", { defaultValue: "Backup deleted" }),
      );
    } catch (error) {
      toast.error(errorText(error));
    }
  };

  return (
    <Dialog open={open} onOpenChange={requestOpenChange}>
      <DialogContent className="max-w-5xl" zIndex="nested">
        <DialogHeader>
          <DialogTitle>
            {t("grokBuild.globalTitle", {
              defaultValue: "Grok Build global config.toml",
            })}
          </DialogTitle>
          <DialogDescription>
            {t("grokBuild.globalHint", {
              defaultValue:
                "Edit the complete live file. Provider switches preserve global, MCP, and unknown settings.",
            })}
          </DialogDescription>
          {path && (
            <p className="break-all font-mono text-xs text-muted-foreground">
              {path} · {source}
              {isDirty
                ? ` · ${t("grokBuild.unsaved", { defaultValue: "unsaved" })}`
                : ""}
            </p>
          )}
        </DialogHeader>

        <div className="max-h-[70vh] space-y-5 overflow-y-auto px-6 py-5">
          {isLoading ? (
            <div className="flex min-h-72 items-center justify-center">
              <Loader2 className="h-6 w-6 animate-spin" />
            </div>
          ) : (
            <JsonEditor
              value={content}
              onChange={updateDraft}
              darkMode={isDarkMode}
              height={400}
              showValidation={false}
              language="plaintext"
            />
          )}

          <section className="space-y-2">
            <div className="flex items-center justify-between gap-3">
              <div>
                <h3 className="text-sm font-semibold">
                  {t("grokBuild.backupTitle", {
                    defaultValue: "Automatic backups",
                  })}
                </h3>
                <p className="text-xs text-muted-foreground">
                  {t("grokBuild.backupWarning", {
                    defaultValue:
                      "Backups may contain API keys. They stay in the local CC Switch configuration directory.",
                  })}
                </p>
              </div>
            </div>
            {backups.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                {t("grokBuild.noBackups", { defaultValue: "No backups yet" })}
              </p>
            ) : (
              <div className="divide-y rounded-md border">
                {backups.map((backup) => (
                  <div
                    key={backup.filename}
                    className="flex items-center justify-between gap-3 px-3 py-2"
                  >
                    <div className="min-w-0">
                      <p className="truncate font-mono text-xs">
                        {backup.filename}
                      </p>
                      <p className="text-xs text-muted-foreground">
                        {new Date(backup.createdAt).toLocaleString()} ·{" "}
                        {backup.sizeBytes} B
                      </p>
                    </div>
                    <div className="flex shrink-0 gap-1">
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        disabled={isLoading || isBusy}
                        onClick={() => restoreBackup(backup)}
                      >
                        <RotateCcw className="mr-1 h-3.5 w-3.5" />
                        {t("grokBuild.restore", { defaultValue: "Restore" })}
                      </Button>
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        disabled={isLoading || isBusy}
                        aria-label={t("grokBuild.deleteBackup", {
                          defaultValue: "Delete backup",
                        })}
                        onClick={() => deleteBackup(backup)}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            disabled={isLoading || !hasLoadedConfig || isBusy}
            onClick={applyPrivacyDraft}
          >
            <ShieldCheck className="mr-2 h-4 w-4" />
            {t("grokBuild.privacyDraft", {
              defaultValue: "Add privacy settings to draft",
            })}
          </Button>
          <Button
            type="button"
            variant="outline"
            disabled={isBusy}
            onClick={() => requestOpenChange(false)}
          >
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            disabled={isLoading || !hasLoadedConfig || isBusy || !isDirty}
            onClick={save}
          >
            {isSaving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            {t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
