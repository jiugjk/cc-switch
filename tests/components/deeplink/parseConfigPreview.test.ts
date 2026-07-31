import { describe, expect, it } from "vitest";
import { parseConfigPreview } from "@/components/deeplink/parseConfigPreview";

const b64 = (text: string): string =>
  Buffer.from(text, "utf-8").toString("base64");

const b64Json = (value: unknown): string => b64(JSON.stringify(value));

describe("parseConfigPreview", () => {
  it("returns null without app or config", () => {
    expect(parseConfigPreview({ config: b64Json({}) })).toBeNull();
    expect(parseConfigPreview({ app: "claude" })).toBeNull();
  });

  it("claude: maps env object to entries", () => {
    const preview = parseConfigPreview({
      app: "claude",
      config: b64Json({
        env: {
          ANTHROPIC_AUTH_TOKEN: "sk-test-token",
          ANTHROPIC_BASE_URL: "https://api.example.com",
        },
      }),
    });
    expect(preview).toEqual({
      app: "claude",
      entries: {
        ANTHROPIC_AUTH_TOKEN: "sk-test-token",
        ANTHROPIC_BASE_URL: "https://api.example.com",
      },
    });
  });

  it("codex: maps auth to entries and config string to tomlText", () => {
    const preview = parseConfigPreview({
      app: "codex",
      config: b64Json({
        auth: { OPENAI_API_KEY: "sk-codex" },
        config: 'model = "gpt-5"',
      }),
    });
    expect(preview).toEqual({
      app: "codex",
      entries: { OPENAI_API_KEY: "sk-codex" },
      tomlText: 'model = "gpt-5"',
    });
  });

  it("gemini: maps flat JSON to entries", () => {
    const preview = parseConfigPreview({
      app: "gemini",
      config: b64Json({
        GEMINI_API_KEY: "g-key",
        GEMINI_BASE_URL: "https://gemini.example.com",
      }),
    });
    expect(preview).toEqual({
      app: "gemini",
      entries: {
        GEMINI_API_KEY: "g-key",
        GEMINI_BASE_URL: "https://gemini.example.com",
      },
    });
  });

  it("grokbuild: configFormat=toml renders the raw TOML document", () => {
    const toml = '[models]\ndefault = "grok-4.5"\napi_key = "secret"';
    const preview = parseConfigPreview({
      app: "grokbuild",
      config: b64(toml),
      configFormat: "toml",
    });
    expect(preview).toEqual({ app: "grokbuild", tomlText: toml });
  });

  it("grokbuild: JSON { config: '<toml>' } renders the TOML string", () => {
    const toml = 'base_url = "https://grok.example.com/v1"';
    const preview = parseConfigPreview({
      app: "grokbuild",
      config: b64Json({ config: toml }),
    });
    expect(preview).toEqual({ app: "grokbuild", tomlText: toml });
  });

  it("grokbuild: falls back to raw text when JSON parsing fails", () => {
    const toml = '[models]\ndefault = "grok-4.5"';
    const preview = parseConfigPreview({
      app: "grokbuild",
      config: b64(toml),
    });
    expect(preview).toEqual({ app: "grokbuild", tomlText: toml });
  });

  it.each(["opencode", "openclaw", "hermes"] as const)(
    "%s: maps flat JSON to entries and stringifies nested values",
    (app) => {
      const preview = parseConfigPreview({
        app,
        config: b64Json({
          apiKey: "sk-additive",
          baseUrl: "https://api.example.com",
          options: { baseURL: "https://api.example.com/v1" },
        }),
      });
      expect(preview).toEqual({
        app,
        entries: {
          apiKey: "sk-additive",
          baseUrl: "https://api.example.com",
          options: '{"baseURL":"https://api.example.com/v1"}',
        },
      });
    },
  );

  it("returns null for invalid base64/JSON payloads without throwing", () => {
    expect(
      parseConfigPreview({ app: "claude", config: "%%%not-base64%%%" }),
    ).toBeNull();
    expect(
      parseConfigPreview({ app: "hermes", config: b64("not json at all") }),
    ).toBeNull();
    // JSON but not an object
    expect(
      parseConfigPreview({ app: "gemini", config: b64Json([1, 2, 3]) }),
    ).toBeNull();
  });
});
