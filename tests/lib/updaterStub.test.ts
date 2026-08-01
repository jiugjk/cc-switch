import { describe, it, expect } from "vitest";
import { checkForUpdate } from "@/lib/updater";

// F-001 回归：本发行版的 checkForUpdate 是无网络、无 @tauri-apps/plugin-updater
// 依赖的桩（该依赖已从 package.json 移除——若源码仍引用它，本文件在模块解析
// 阶段就会失败）。桩必须恒返回 up-to-date，保证徽标/横幅永不出现。
describe("lib/updater (distribution stub)", () => {
  it("always reports up-to-date without touching the updater plugin", async () => {
    await expect(checkForUpdate({ timeout: 1 })).resolves.toEqual({
      status: "up-to-date",
    });
  });
});
