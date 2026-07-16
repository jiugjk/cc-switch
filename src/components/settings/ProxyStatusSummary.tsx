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

  // 代理从未启动过且无任何路由/流量痕迹时，不占用 General 页空间。
  // get_proxy_status 在停止时仍返回 Default 对象，不能仅用 `!status`。
  if (
    !status ||
    (!status.running &&
      !status.last_route_protocol &&
      !status.last_fallback_reason &&
      (status.total_requests ?? 0) === 0)
  ) {
    return null;
  }

  const listenAddress =
    status.address && status.port ? `${status.address}:${status.port}` : "—";

  const rows: {
    key: string;
    label: string;
    value: string;
    mono?: boolean;
  }[] = [
    {
      key: "listen",
      label: t("settings.proxyStatus.listenAddress"),
      value: listenAddress,
      mono: true,
    },
    {
      key: "logical",
      label: t("settings.proxyStatus.logicalProvider"),
      value: status.current_provider ?? "—",
    },
    {
      key: "takeover",
      label: t("settings.proxyStatus.routedThroughCcSwitch"),
      value: isTakeoverActive
        ? t("settings.proxyStatus.yes")
        : t("settings.proxyStatus.no"),
    },
    {
      key: "protocol",
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
      key: "continuation",
      label: t("settings.proxyStatus.continuation"),
      value: status.last_route_continuation
        ? t(
            `settings.proxyStatus.continuationValue.${status.last_route_continuation}`,
            { defaultValue: status.last_route_continuation },
          )
        : "—",
    },
    {
      key: "upstream",
      label: t("settings.proxyStatus.lastUpstream"),
      value: status.last_masked_upstream ?? "—",
      mono: true,
    },
  ];

  if (status.last_fallback_reason) {
    // Backend stores `quota_exhausted:{app}:{provider}` (no credentials).
    const parts = status.last_fallback_reason.split(":");
    const providerHint =
      parts[0] === "quota_exhausted" && parts.length >= 3
        ? parts.slice(2).join(":")
        : null;
    rows.push({
      key: "fallback",
      label: t("settings.proxyStatus.lastFallback"),
      value: providerHint
        ? t("settings.proxyStatus.quotaFallbackWithProvider", {
            provider: providerHint,
            defaultValue: `Quota exhausted → switched from ${providerHint}`,
          })
        : t("settings.proxyStatus.quotaFallback", {
            defaultValue: "Quota exhausted → switched provider",
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
                ? "text-green-500 motion-safe:animate-pulse"
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
              key={row.key}
              className="flex flex-col gap-0.5 rounded-lg bg-background/60 p-2.5"
            >
              <dt className="flex items-center gap-1.5 text-xs text-muted-foreground">
                {row.key === "protocol" ? (
                  <Radio className="h-3 w-3" />
                ) : row.key === "takeover" ? (
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
