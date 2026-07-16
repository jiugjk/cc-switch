import { useTranslation } from "react-i18next";
import { ShieldCheck } from "lucide-react";
import type { SettingsFormState } from "@/hooks/useSettings";
import { ToggleRow } from "@/components/ui/toggle-row";

interface CustomApiQuotaSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

/**
 * D2 —「使用自定义 API 时不依赖 ChatGPT 官方额度」开关（General 页）。
 *
 * 开启后：Codex 链路的模型请求把内置官方 Codex 供应商从候选链中剔除，
 * 使自定义 API 请求不会在失败时静默回退到官方额度（仅当剔除后仍有
 * 非官方供应商时生效，绝不清空候选链）。
 *
 * 约束（见报告限制说明）：
 * - 只作用于 Codex CLI 及任何以本地代理为 base_url 的客户端；
 * - 微软商店版 ChatGPT 桌面应用的 UI 内流量无法合规接管，此开关不影响它；
 * - 不清除/覆盖已有 ChatGPT 登录态，不把官方凭据发给第三方（凭据隔离由
 *   转发层 per-provider 认证保证）。
 *
 * 默认关闭，向后兼容。
 */
export function CustomApiQuotaSettings({
  settings,
  onChange,
}: CustomApiQuotaSettingsProps) {
  const { t } = useTranslation();

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <ShieldCheck className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">
          {t("settings.customApiQuota.title")}
        </h3>
      </div>

      <ToggleRow
        icon={<ShieldCheck className="h-4 w-4 text-indigo-500" />}
        title={t("settings.decoupleOfficialQuota")}
        description={t("settings.decoupleOfficialQuotaDescription")}
        checked={settings.decoupleOfficialQuota ?? false}
        onCheckedChange={(value) => onChange({ decoupleOfficialQuota: value })}
      />
    </section>
  );
}
