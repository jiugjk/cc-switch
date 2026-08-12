import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";

const flattenLocale = (value: unknown, prefix = ""): string[] => {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return Object.entries(value).flatMap(([key, child]) =>
      flattenLocale(child, prefix ? `${prefix}.${key}` : key),
    );
  }
  return prefix ? [prefix] : [];
};

const localeEntries = [
  ["zh", zh],
  ["ja", ja],
  ["zh-TW", zhTW],
] as const;

describe("i18n locale parity", () => {
  const baseKeys = new Set(flattenLocale(en));

  for (const [name, locale] of localeEntries) {
    it(`${name} has exactly the English translation keys`, () => {
      const keys = new Set(flattenLocale(locale));
      const missing = [...baseKeys].filter((key) => !keys.has(key));
      const extra = [...keys].filter((key) => !baseKeys.has(key));

      expect({ missing, extra }).toEqual({ missing: [], extra: [] });
    });
  }
});
