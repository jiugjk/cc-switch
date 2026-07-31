import type { DeepLinkAppId, DeepLinkImportRequest } from "@/lib/api/deeplink";
import { decodeBase64Utf8 } from "@/lib/utils/base64";

/**
 * Normalized preview of a deep link embedded config, mirroring how the
 * backend consumes each app's payload (src-tauri/src/deeplink/provider.rs):
 * - claude: JSON `{ env: {...} }` -> entries
 * - codex: JSON `{ auth: {...}, config: "<TOML>" }` -> entries + tomlText
 * - gemini: flat JSON -> entries
 * - grokbuild: JSON `{ config: "<TOML>" }` or a raw TOML document
 *   (configFormat=toml) -> tomlText; flat JSON fallback -> entries
 * - opencode / openclaw / hermes: additive flat JSON
 *   (apiKey / baseUrl / options...) -> entries
 */
export interface ConfigPreview {
  app: DeepLinkAppId;
  entries?: Record<string, string>;
  tomlText?: string;
}

const stringifyValue = (value: unknown): string =>
  typeof value === "string" ? value : JSON.stringify(value);

const toEntries = (obj: Record<string, unknown>): Record<string, string> => {
  const entries: Record<string, string> = {};
  for (const [key, value] of Object.entries(obj)) {
    entries[key] = stringifyValue(value);
  }
  return entries;
};

const tryParseJsonObject = (text: string): Record<string, unknown> | null => {
  try {
    const value: unknown = JSON.parse(text);
    if (value && typeof value === "object" && !Array.isArray(value)) {
      return value as Record<string, unknown>;
    }
    return null;
  } catch {
    return null;
  }
};

/**
 * Parse the embedded (Base64) config of a deep link request into a
 * displayable preview. Returns null when there is nothing to preview
 * (missing app/config or undecodable payload). Never throws.
 */
export function parseConfigPreview(
  request: Pick<DeepLinkImportRequest, "app" | "config" | "configFormat">,
): ConfigPreview | null {
  const { app, config } = request;
  if (!app || !config) return null;

  let decoded: string;
  try {
    decoded = decodeBase64Utf8(config);
  } catch (e) {
    console.error("Failed to decode config:", e);
    return null;
  }

  const parsed = tryParseJsonObject(decoded);

  switch (app) {
    case "claude":
      // Claude: { env: { ANTHROPIC_AUTH_TOKEN: ..., ... } }
      if (!parsed) return null;
      return {
        app,
        entries: toEntries((parsed.env as Record<string, unknown>) || {}),
      };
    case "codex":
      // Codex: { auth: { OPENAI_API_KEY: ... }, config: "<TOML string>" }
      if (!parsed) return null;
      return {
        app,
        entries: toEntries((parsed.auth as Record<string, unknown>) || {}),
        tomlText: typeof parsed.config === "string" ? parsed.config : "",
      };
    case "gemini":
      // Gemini: flat JSON { GEMINI_API_KEY: ..., GEMINI_BASE_URL: ... }
      if (!parsed) return null;
      return { app, entries: toEntries(parsed) };
    case "grokbuild":
      // Grok Build: raw TOML document (configFormat=toml, or JSON parse
      // failure as fallback), JSON { config: "<TOML>" }, or flat JSON.
      if (request.configFormat === "toml" || !parsed) {
        return { app, tomlText: decoded };
      }
      if (typeof parsed.config === "string") {
        return { app, tomlText: parsed.config };
      }
      return { app, entries: toEntries(parsed) };
    case "opencode":
    case "openclaw":
    case "hermes":
      // Additive flat JSON: apiKey/api_key, baseUrl/base_url, options...
      if (!parsed) return null;
      return { app, entries: toEntries(parsed) };
    default:
      return null;
  }
}
