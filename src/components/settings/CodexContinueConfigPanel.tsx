import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { settingsApi, type CodexContinueConfig } from "@/lib/api/settings";

const DEFAULT_CODEX_CONTINUE_CONFIG: CodexContinueConfig = {
  enabled: true,
  maxContinuations: 8,
  step: 518,
  marker:
    "We need continue thinking. Do not summarize; continue from the previous reasoning state.",
};

type SuccessToastKind = "toggle" | "saved";

export function CodexContinueConfigPanel() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<CodexContinueConfig>(
    DEFAULT_CODEX_CONTINUE_CONFIG,
  );
  const [draft, setDraft] = useState<CodexContinueConfig>(
    DEFAULT_CODEX_CONTINUE_CONFIG,
  );
  const [isLoading, setIsLoading] = useState(true);
  const configRef = useRef(DEFAULT_CODEX_CONTINUE_CONFIG);
  const committedConfigRef = useRef(DEFAULT_CODEX_CONTINUE_CONFIG);
  const latestIntentRef = useRef(0);
  const draftRevisionRef = useRef(0);
  const writeQueueRef = useRef<Promise<void>>(Promise.resolve());
  const isMountedRef = useRef(true);

  useEffect(() => {
    isMountedRef.current = true;
    settingsApi
      .getCodexContinueConfig()
      .then((loaded) => {
        configRef.current = loaded;
        committedConfigRef.current = loaded;
        setConfig(loaded);
        setDraft(loaded);
      })
      .catch((e) => console.error("Failed to load CodexCont config:", e))
      .finally(() => {
        if (isMountedRef.current) setIsLoading(false);
      });
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  const handleChange = (
    updates: Partial<CodexContinueConfig>,
    preserveAdvancedDraft = false,
    successToastKind: SuccessToastKind = "toggle",
  ) => {
    // Tauri commands may complete out of order. Build every intent from the
    // latest optimistic value, then serialize the complete snapshots so the
    // backend always finishes in the same order as the user's actions.
    const newConfig = { ...configRef.current, ...updates };
    const intent = ++latestIntentRef.current;
    const draftRevision = ++draftRevisionRef.current;
    configRef.current = newConfig;
    setConfig(newConfig);
    setDraft((previousDraft) =>
      preserveAdvancedDraft
        ? { ...previousDraft, enabled: newConfig.enabled }
        : newConfig,
    );

    const write = writeQueueRef.current.then(async () => {
      try {
        await settingsApi.setCodexContinueConfig(newConfig);
        committedConfigRef.current = newConfig;
        if (isMountedRef.current && intent === latestIntentRef.current) {
          toast.success(
            successToastKind === "saved"
              ? t("settings.advanced.codexContinue.savedToast", {
                  defaultValue: "续写配置保存成功",
                })
              : newConfig.enabled
                ? t("settings.advanced.codexContinue.enabledToast", {
                    defaultValue: "CodexCont 自动续写已启用",
                  })
                : t("settings.advanced.codexContinue.disabledToast", {
                    defaultValue: "CodexCont 自动续写已关闭",
                  }),
            { closeButton: true },
          );
        }
      } catch (error) {
        console.error("Failed to save CodexCont config:", error);
        if (!isMountedRef.current || intent !== latestIntentRef.current) return;

        const committed = committedConfigRef.current;
        configRef.current = committed;
        setConfig(committed);
        setDraft((previousDraft) => {
          // A toggle never owns the unsaved advanced draft. Likewise, if the
          // user edited fields while an advanced save was in flight, preserve
          // that newer draft and roll back only the committed enabled state.
          if (
            preserveAdvancedDraft ||
            draftRevisionRef.current !== draftRevision
          ) {
            return { ...previousDraft, enabled: committed.enabled };
          }
          return committed;
        });
        toast.error(String(error));
      }
    });
    // Each task handles its own failure; keep a permanently usable queue even
    // when an individual persistence call rejects.
    writeQueueRef.current = write;
    return write;
  };

  const handleSaveAdvanced = async () => {
    const maxContinuations = Math.min(
      32,
      Math.max(0, Math.floor(draft.maxContinuations)),
    );
    const step = Math.max(3, Math.floor(draft.step));
    const marker = draft.marker.trim() || DEFAULT_CODEX_CONTINUE_CONFIG.marker;
    await handleChange({ maxContinuations, step, marker }, false, "saved");
  };

  const updateDraft = (updates: Partial<CodexContinueConfig>) => {
    draftRevisionRef.current += 1;
    setDraft((previousDraft) => ({ ...previousDraft, ...updates }));
  };

  if (isLoading) return null;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="space-y-0.5">
          <Label>
            {t("settings.advanced.codexContinue.enabled", {
              defaultValue: "启用自动续写",
            })}
          </Label>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.codexContinue.enabledDescription", {
              defaultValue:
                "关闭后 Codex 请求仍通过 CC Switch 路由，但不再做 CodexCont 折叠续写。",
            })}
          </p>
        </div>
        <Switch
          checked={config.enabled}
          onCheckedChange={(checked) =>
            handleChange({ enabled: checked }, true)
          }
        />
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        <div className="space-y-2">
          <Label htmlFor="codex-continue-max">
            {t("settings.advanced.codexContinue.maxContinuations", {
              defaultValue: "最大续写轮数",
            })}
          </Label>
          <Input
            id="codex-continue-max"
            type="number"
            min={0}
            max={32}
            value={draft.maxContinuations}
            onChange={(event) =>
              updateDraft({ maxContinuations: Number(event.target.value) })
            }
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="codex-continue-step">
            {t("settings.advanced.codexContinue.step", {
              defaultValue: "截断步长",
            })}
          </Label>
          <Input
            id="codex-continue-step"
            type="number"
            min={3}
            value={draft.step}
            onChange={(event) =>
              updateDraft({ step: Number(event.target.value) })
            }
          />
        </div>
      </div>

      <div className="space-y-2">
        <Label htmlFor="codex-continue-marker">
          {t("settings.advanced.codexContinue.marker", {
            defaultValue: "续写提示",
          })}
        </Label>
        <Textarea
          id="codex-continue-marker"
          value={draft.marker}
          onChange={(event) => updateDraft({ marker: event.target.value })}
          rows={3}
        />
        <p className="text-xs text-muted-foreground">
          {t("settings.advanced.codexContinue.markerDescription", {
            defaultValue:
              "用于触发下一轮 reasoning continuation；环境变量仍可临时覆盖这些参数。",
          })}
        </p>
      </div>

      <div className="flex justify-end">
        <Button size="sm" onClick={() => void handleSaveAdvanced()}>
          {t("common.save", { defaultValue: "保存" })}
        </Button>
      </div>
    </div>
  );
}
