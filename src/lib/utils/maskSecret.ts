/**
 * Unified display-only masking for secrets (API keys, tokens, passwords).
 *
 * Masking is cosmetic: import/submit flows always use the original values.
 */

const SENSITIVE_KEY_PATTERNS = ["TOKEN", "KEY", "SECRET", "PASSWORD"];

/**
 * Case-insensitive check whether a config key looks sensitive.
 * "KEY" also covers apiKey / API_KEY variants.
 */
export function isSensitiveKey(key: string): boolean {
  const upper = key.toUpperCase();
  return SENSITIVE_KEY_PATTERNS.some((pattern) => upper.includes(pattern));
}

/**
 * Mask a secret value for display:
 * - length <= 8 (including empty): fully masked as "***"
 * - longer: 4-char prefix + fixed 8-star mask + 2-char suffix
 *   (constant mask width, so the real length is not fully inferable)
 */
export function maskSecret(value: string): string {
  if (value.length <= 8) {
    return "***";
  }
  return `${value.slice(0, 4)}${"*".repeat(8)}${value.slice(-2)}`;
}

/**
 * Mask `value` only when `key` looks sensitive; otherwise return it unchanged.
 */
export function maskSensitiveValue(key: string, value: string): string {
  return isSensitiveKey(key) ? maskSecret(value) : value;
}
