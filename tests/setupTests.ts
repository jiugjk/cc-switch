import "@testing-library/jest-dom";
import { afterAll, afterEach, beforeAll, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { server } from "./msw/server";
import { resetProviderState } from "./msw/state";
import "./msw/tauriMocks";

beforeAll(async () => {
  server.listen({ onUnhandledRequest: "warn" });
  await i18n.use(initReactI18next).init({
    lng: "zh",
    fallbackLng: "zh",
    resources: {
      zh: { translation: {} },
      en: { translation: {} },
    },
    interpolation: {
      escapeValue: false,
    },
  });
});

afterEach(() => {
  cleanup();
  resetProviderState();
  server.resetHandlers();
  // Fake timers left on by a sibling file will freeze waitFor under parallel
  // load; always restore real clocks between tests.
  vi.useRealTimers();
  vi.clearAllMocks();
  try {
    localStorage.clear();
  } catch {
    // jsdom / polyfill edge cases
  }
  try {
    sessionStorage.clear();
  } catch {
    // ignore
  }
});

afterAll(() => {
  server.close();
});
