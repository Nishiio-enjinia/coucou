// Shared catalogs live in /locales. English is the source and the fallback.
// Add every new key to both locales/en.json and locales/fr.json.
// "system" follows the OS. "en" and "fr" are explicit. Placeholders are {name}.

import enJson from "../../../locales/en.json";
import frJson from "../../../locales/fr.json";

export type LocaleId = "en" | "fr";
export type LanguagePref = "system" | LocaleId;

const en = enJson as Record<string, string>;
const fr = frJson as Record<string, string>;

function systemLocale(): LocaleId {
  return navigator.language.toLowerCase().startsWith("fr") ? "fr" : "en";
}

let active: LocaleId = systemLocale();

export function applyLanguage(pref: LanguagePref | undefined): void {
  active = pref === "en" || pref === "fr" ? pref : systemLocale();
  document.documentElement.lang = active;
}

export function t(key: string, vars?: Record<string, string | number>): string {
  const table = active === "fr" ? fr : en;
  let value = table[key] ?? en[key];
  if (value == null) {
    if (import.meta.env.DEV) console.warn(`[i18n] missing key: ${key}`);
    value = key;
  }
  if (vars) {
    for (const [name, replacement] of Object.entries(vars)) {
      value = value.replaceAll(`{${name}}`, String(replacement));
    }
  }
  return value;
}
