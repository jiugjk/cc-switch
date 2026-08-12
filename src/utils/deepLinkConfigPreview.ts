import { parse as parseToml, stringify as stringifyToml } from "smol-toml";
import type { DeepLinkImportRequest } from "@/lib/api/deeplink";
import { decodeBase64Utf8 } from "@/lib/utils/base64";
import { isSensitiveConfigKey, maskSensitiveValue } from "@/utils/deeplinkRisk";

export interface ParsedDeepLinkConfig {
  type: "claude" | "codex" | "gemini" | "grokbuild" | "generic";
  env?: Record<string, string>;
  auth?: Record<string, string>;
  tomlConfig?: string | null;
  configText?: string | null;
  /** The payload was intentionally not parsed because it exceeds the preview budget. */
  oversized?: boolean;
}

/**
 * Keep synchronous parsing on the confirmation-dialog path bounded. This is a
 * preview limit only; the backend still receives the original payload after
 * the user confirms the import.
 */
export const MAX_PREVIEW_CONFIG_BYTES = 64 * 1024;
const MAX_PREVIEW_BASE64_CHARS =
  Math.ceil(MAX_PREVIEW_CONFIG_BYTES / 3) * 4 + 4;

const utf8ByteLength = (value: string): number =>
  new TextEncoder().encode(value).byteLength;

const exceedsPreviewLimit = (value: string): boolean =>
  value.length > MAX_PREVIEW_CONFIG_BYTES ||
  utf8ByteLength(value) > MAX_PREVIEW_CONFIG_BYTES;

const maskStructuredSecrets = (
  value: unknown,
  key = "",
  inheritedSensitive = false,
): unknown => {
  const sensitive = inheritedSensitive || isSensitiveConfigKey(key);
  if (typeof value === "string") {
    return sensitive ? maskSensitiveValue(value) : value;
  }
  if (Array.isArray(value)) {
    return value.map((item) => maskStructuredSecrets(item, key, sensitive));
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(
        ([childKey, childValue]) => [
          childKey,
          maskStructuredSecrets(childValue, childKey, sensitive),
        ],
      ),
    );
  }
  return value;
};

const sanitizeTomlForPreview = (configToml: string): string => {
  const parsed = parseToml(configToml) as Record<string, unknown>;
  return `${stringifyToml(maskStructuredSecrets(parsed) as Record<string, unknown>).trim()}\n`;
};

export function parseDeepLinkConfigPreview(
  request: Pick<DeepLinkImportRequest, "app" | "config" | "configFormat">,
): ParsedDeepLinkConfig | null {
  if (!request.config) return null;

  try {
    // A valid Base64 encoding of a 64 KiB payload cannot exceed this bound.
    // Reject clearly oversized links before decoding them into another large
    // string; the exact UTF-8 byte check below handles the boundary cases.
    if (request.config.length > MAX_PREVIEW_BASE64_CHARS) {
      return { type: "generic", configText: null, oversized: true };
    }

    const decoded = decodeBase64Utf8(request.config);
    const format = request.configFormat?.trim().toLowerCase();

    // Check before invoking either TOML or JSON parsing. `config` is supplied
    // by an untrusted deep link and a very large synchronous parse would block
    // the WebView before the user can reject the import.
    if (exceedsPreviewLimit(decoded)) {
      return { type: "generic", configText: null, oversized: true };
    }

    if (request.app === "grokbuild" && format === "toml") {
      return {
        type: "grokbuild",
        tomlConfig: sanitizeTomlForPreview(decoded),
      };
    }

    if (format === "toml") {
      return {
        type: "generic",
        configText: sanitizeTomlForPreview(decoded),
      };
    }

    const parsed = JSON.parse(decoded) as Record<string, unknown>;
    if (request.app === "claude") {
      return {
        type: "claude",
        env: (parsed.env as Record<string, string>) || {},
      };
    }
    if (request.app === "codex") {
      const config = typeof parsed.config === "string" ? parsed.config : "";
      if (config && exceedsPreviewLimit(config)) {
        return {
          type: "codex",
          auth: (parsed.auth as Record<string, string>) || {},
          tomlConfig: null,
          oversized: true,
        };
      }
      return {
        type: "codex",
        auth: (parsed.auth as Record<string, string>) || {},
        tomlConfig: config ? sanitizeTomlForPreview(config) : "",
      };
    }
    if (request.app === "gemini") {
      return {
        type: "gemini",
        env: parsed as Record<string, string>,
      };
    }
    if (request.app === "grokbuild") {
      const config =
        typeof parsed.config === "string"
          ? parsed.config
          : stringifyToml(parsed);
      if (exceedsPreviewLimit(config)) {
        return { type: "grokbuild", tomlConfig: null, oversized: true };
      }
      return {
        type: "grokbuild",
        tomlConfig: sanitizeTomlForPreview(config),
      };
    }
    const configText = JSON.stringify(maskStructuredSecrets(parsed), null, 2);
    if (typeof configText === "string" && exceedsPreviewLimit(configText)) {
      return { type: "generic", configText: null, oversized: true };
    }
    return {
      type: "generic",
      configText,
    };
  } catch (error) {
    console.error("Failed to parse deep link config preview:", error);
    return null;
  }
}
