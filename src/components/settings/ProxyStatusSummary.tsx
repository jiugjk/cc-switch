import { useTranslation } from "react-i18next";
import { Activity, Network, Radio, Shuffle } from "lucide-react";
import { useProxyStatus } from "@/hooks/useProxyStatus";

/**
 * D1 — 只读代理运行状态摘要（General 页）。
 *
 * 目的：消除「Codex 配置里的 Base URL 没变化 = 代理没生效」的误解。
 * 代理是「接管」语义——它把 Codex 的 live 配置重写为指向本地监听地址，
 * 逻辑上游 base_url 保持用户填写值不变，实际连接先走本地代理入口，再由
 * 代理按路由选择最终上游。这里把三态分别展示：
 *   - 本地监听地址（Codex 实际首先连接的地址）
 *   - 最近一次成功转发的脱敏上游（scheme://host[:port]，绝不含 Key/Token/查询）
 *   - 实际转发协议 / continuation 档位
 *
 * 全部只读，不改写任何持久化配置。
 */
export function ProxyStatusSummary() {
  const { t } = useTranslation();
  const { status, isRunning, isTakeoverActive } = useProxyStatus();

  // 代理从未启动过且无任何状态时，不占用 General 页空间。
  if (!status) return null;

  const listenAddress =
    status.address && status.port ? `${status.address}:${status.port}` : "—";

  const rows: { label: string; value: string; mono?: boolean }[] = [
    {
      label: t("settings.proxyStatus.listenAddress"),
      value: listenAddress,
      mono: true,
    },
    {
      label: t("settings.proxyStatus.logicalProvider"),
      value: status.current_provider ?? "—",
    },
    {
      label: t("settings.proxyStatus.routedThroughCcSwitch"),
      value: isTakeoverActive
        ? t("settings.proxyStatus.yes")
        : t("settings.proxyStatus.no"),
    },
    {
      label: t("settings.proxyStatus.protocol"),
      value: status.last_route_protocol
        ? t(
            `settings.proxyStatus.protocolValue.${status.last_route_protocol}`,
            {
              defaultValue: status.last_route_protocol,
            },
          )
        : "—",
    },
    {
      label: t("settings.proxyStatus.continuation"),
      value: status.last_route_continuation
        ? t(
            `settings.proxyStatus.continuationValue.${status.last_route_continuation}`,
            { defaultValue: status.last_route_continuation },
          )
        : "—",
    },
    {
      label: t("settings.proxyStatus.lastUpstream"),
      value: status.last_masked_upstream ?? "—",
      mono: true,
    },
  ];

  if (status.last_fallback_reason) {
    rows.push({
      label: t("settings.proxyStatus.lastFallback"),
      value: t("settings.proxyStatus.quotaFallback", {
        defaultValue: "配额耗尽切换",
      }),
    });
  }

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <Network className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">
          {t("settings.proxyStatus.title")}
        </h3>
      </div>

      <div className="rounded-xl border border-border bg-card/50 p-4 space-y-3">
        <div className="flex items-center gap-2">
          <Activity
            className={`h-4 w-4 ${
              isRunning
                ? "text-green-500 animate-pulse"
                : "text-muted-foreground"
            }`}
          />
          <span className="text-sm font-medium">
            {isRunning
              ? t("settings.proxyStatus.running")
              : t("settings.proxyStatus.stopped")}
          </span>
        </div>

        <p className="text-xs text-muted-foreground leading-relaxed">
          {t("settings.proxyStatus.explainer")}
        </p>

        <dl className="grid grid-cols-1 gap-2 sm:grid-cols-2">
          {rows.map((row) => (
            <div
              key={row.label}
              className="flex flex-col gap-0.5 rounded-lg bg-background/60 p-2.5"
            >
              <dt className="flex items-center gap-1.5 text-xs text-muted-foreground">
                {row.label === t("settings.proxyStatus.protocol") ? (
                  <Radio className="h-3 w-3" />
                ) : row.label ===
                  t("settings.proxyStatus.routedThroughCcSwitch") ? (
                  <Shuffle className="h-3 w-3" />
                ) : null}
                {row.label}
              </dt>
              <dd
                className={`text-sm ${
                  row.mono ? "font-mono text-xs break-all" : ""
                }`}
              >
                {row.value}
              </dd>
            </div>
          ))}
        </dl>
      </div>
    </section>
  );
}
