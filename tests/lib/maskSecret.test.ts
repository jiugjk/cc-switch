import { describe, expect, it } from "vitest";
import {
  isSensitiveKey,
  maskSecret,
  maskSensitiveValue,
} from "@/lib/utils/maskSecret";

describe("maskSecret", () => {
  it("fully masks short values (<= 8 chars) as ***", () => {
    expect(maskSecret("a")).toBe("***");
    expect(maskSecret("sk-1234")).toBe("***");
    expect(maskSecret("12345678")).toBe("***");
  });

  it("handles the empty string", () => {
    expect(maskSecret("")).toBe("***");
  });

  it("keeps a 4-char prefix and 2-char suffix for long values", () => {
    const secret = "sk-abcdef1234567890";
    const masked = maskSecret(secret);
    expect(masked).toBe(`sk-a${"*".repeat(8)}90`);
  });

  it("never reveals the middle of the secret and uses a constant mask width", () => {
    const secret = "sk-verylongsecretvalue-with-middle-part";
    const masked = maskSecret(secret);
    expect(masked).not.toContain(secret.slice(4, -2));
    expect(masked.length).toBe(4 + 8 + 2);
    // A different long secret produces the same masked length
    expect(maskSecret("x".repeat(100)).length).toBe(masked.length);
  });
});

describe("isSensitiveKey", () => {
  it("matches sensitive keys case-insensitively", () => {
    expect(isSensitiveKey("token")).toBe(true);
    expect(isSensitiveKey("ApiKey")).toBe(true);
    expect(isSensitiveKey("API_KEY")).toBe(true);
    expect(isSensitiveKey("PASSWORD")).toBe(true);
    expect(isSensitiveKey("secret")).toBe(true);
    expect(isSensitiveKey("ANTHROPIC_AUTH_TOKEN")).toBe(true);
  });

  it("does not match non-sensitive keys", () => {
    expect(isSensitiveKey("baseUrl")).toBe(false);
    expect(isSensitiveKey("endpoint")).toBe(false);
    expect(isSensitiveKey("model")).toBe(false);
  });
});

describe("maskSensitiveValue", () => {
  it("masks values for sensitive keys", () => {
    expect(maskSensitiveValue("apiKey", "12345678")).toBe("***");
    expect(maskSensitiveValue("ANTHROPIC_AUTH_TOKEN", "sk-abcdef1234")).toBe(
      `sk-a${"*".repeat(8)}34`,
    );
  });

  it("passes through values for non-sensitive keys", () => {
    expect(maskSensitiveValue("baseUrl", "https://api.example.com")).toBe(
      "https://api.example.com",
    );
  });
});
