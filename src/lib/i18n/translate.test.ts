import { describe, expect, it } from "vitest";
import de from "./de.json";
import en from "./en.json";
import { resolveLocale, translate } from "./translate";

describe("i18n", () => {
  it("resolves locale tags", () => {
    expect(resolveLocale("de-AT")).toBe("de");
    expect(resolveLocale("en_US")).toBe("en");
    expect(resolveLocale("fr")).toBe("en");
    expect(resolveLocale(undefined)).toBe("en");
  });

  it("replaces parameters", () => {
    expect(translate("de", "home.comingSoon", { phase: 2 })).toBe("Folgt in Phase 2");
    expect(translate("en", "home.comingSoon")).toBe("Coming in phase {phase}");
  });

  it("has the same keys in every language", () => {
    expect(Object.keys(en).sort()).toEqual(Object.keys(de).sort());
  });
});
