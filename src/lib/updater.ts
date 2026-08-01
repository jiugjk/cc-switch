import { getVersion } from "@tauri-apps/api/app";

export type UpdateChannel = "stable" | "beta";

export interface UpdateInfo {
  currentVersion: string;
  availableVersion: string;
  notes?: string;
  pubDate?: string;
}

export interface CheckOptions {
  timeout?: number;
  channel?: UpdateChannel;
}

export async function getCurrentVersion(): Promise<string> {
  try {
    return await getVersion();
  } catch {
    return "";
  }
}

/**
 * 本发行版已禁用应用内自动更新（无签名密钥、无 latest.json 更新通道），
 * 不再引入 @tauri-apps/plugin-updater、不发起任何网络检查。
 * 保留函数签名与返回类型，UpdateContext / UpdateBadge 无需分叉：
 * 恒返回 up-to-date → hasUpdate 恒为 false，更新徽标与横幅永不出现。
 * 手动更新走 check_for_updates 命令（打开本发行版的 GitHub 发布页）。
 */
export async function checkForUpdate(
  _opts: CheckOptions = {},
): Promise<
  { status: "up-to-date" } | { status: "available"; info: UpdateInfo }
> {
  return { status: "up-to-date" };
}
