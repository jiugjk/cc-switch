import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { settingsApi, type AppId } from "@/lib/api";
import type { VisibleApps } from "@/types";

/** 顶部切换栏「已安装」探测结果：true=已装，false=未装，缺失=未知（按已装处理） */
export type InstalledApps = Partial<Record<AppId, boolean>>;

export const INSTALLED_APPS_QUERY_KEY = ["installed-apps"] as const;

/** 探测结果视为新鲜的时长：窗口反复聚焦时在该窗口期内不重复探测（节流）。 */
const INSTALLED_APPS_STALE_TIME_MS = 30_000;

/** 探测各应用本地安装状态（仅本地 --version / 配置目录 / 包身份探测，无网络请求）。
 *  单个工具探测失败只影响该工具自身字段（version 为空 → false），
 *  整个调用失败由 React Query 保留上次数据，不清空其他应用的结果。 */
export async function probeInstalledApps(): Promise<InstalledApps> {
  const [tools, desktopInstalled, codexDesktopInstalled] = await Promise.all([
    settingsApi.getToolVersions(undefined, undefined, false),
    // Claude Desktop 无 CLI，可用性以配置目录存在与否近似；探测异常时视为已安装。
    settingsApi.isClaudeDesktopInstalled().catch(() => true),
    // MS Store 版 Codex 桌面应用（仅 Windows）。它是 CLI 探测之外的补充 OR 信号：
    // 探测异常按 false 处理即可——CLI 在装时按钮仍显示，不会因此误隐藏。
    settingsApi.isCodexDesktopInstalled().catch(() => false),
  ]);
  const next: InstalledApps = {
    "claude-desktop": desktopInstalled,
  };
  for (const tool of tools) {
    next[tool.name as AppId] = Boolean(tool.version);
  }
  // 任一有效 Codex 客户端（CLI 或桌面应用）在装即显示 Codex 入口；
  // 两者官方共享 ~/.codex，现有配置管理对桌面应用同样生效。
  if (codexDesktopInstalled) {
    next.codex = true;
  }
  return next;
}

/** 最终展示 = 用户可见性设置 ∧ 本地已安装。未知（探测未返回/字段缺失）按已安装处理，
 *  宁可多显示，不误隐藏。 */
export function mergeShownApps(
  visibleApps: VisibleApps,
  installedApps: InstalledApps | null,
): VisibleApps {
  if (!installedApps) return visibleApps;
  const merged = { ...visibleApps };
  for (const app of Object.keys(merged) as AppId[]) {
    merged[app] = merged[app] && installedApps[app] !== false;
  }
  return merged;
}

/** 顶部切换栏「已安装且可用」检测。
 *  启动时后台探测一次；窗口重新聚焦/从托盘恢复时（Tauri onFocusChanged 及
 *  React Query 自带的 visibilitychange 聚焦刷新）重新探测，使运行期间新装/卸载
 *  的应用无需重启即可更新按钮。stale: true 过滤 + staleTime 节流反复聚焦；
 *  React Query 自动合并并发探测。结果返回前为 null → 按 visibleApps 原样显示，
 *  避免先显示后隐藏的闪烁反转；探测失败保留上次数据——宁可多显示，不误隐藏。 */
export function useInstalledApps(): InstalledApps | null {
  const queryClient = useQueryClient();
  const { data } = useQuery({
    queryKey: INSTALLED_APPS_QUERY_KEY,
    queryFn: probeInstalledApps,
    staleTime: INSTALLED_APPS_STALE_TIME_MS,
    refetchOnWindowFocus: true,
  });

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;

    const setupFocusListener = async () => {
      try {
        unlisten = await getCurrentWindow().onFocusChanged(
          ({ payload: focused }) => {
            if (!active || !focused) return;
            void queryClient.refetchQueries({
              queryKey: INSTALLED_APPS_QUERY_KEY,
              stale: true,
            });
          },
        );
      } catch (error) {
        console.error(
          "[useInstalledApps] Failed to listen window focus",
          error,
        );
      }
    };

    void setupFocusListener();
    return () => {
      active = false;
      unlisten?.();
    };
  }, [queryClient]);

  return data ?? null;
}
