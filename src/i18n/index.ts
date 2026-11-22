/**
 * The languages of the window. The English text in the source is the key:
 * `t("Save rules")` shows the translation of exactly that text, or the text
 * itself in English and where a translation is missing. A translation lives
 * in `<code>.json` next to this file, and `scripts/check-i18n.mjs` (part of
 * `npm run lint`) fails when one lacks a text, keeps one that is gone, or
 * changes a `{placeholder}`.
 */
import de from "./de.json";
import es from "./es.json";
import fr from "./fr.json";
import pl from "./pl.json";
import ptBR from "./pt-BR.json";
import zhHans from "./zh-Hans.json";

/** Every language of the project, under its own name. */
const ALL_LANGUAGES = [
  { code: "en", name: "English" },
  { code: "de", name: "Deutsch" },
  { code: "es", name: "Español" },
  { code: "fr", name: "Français" },
  { code: "pl", name: "Polski" },
  { code: "pt-BR", name: "Português (Brasil)" },
  { code: "zh-Hans", name: "简体中文" },
] as const;

export type LanguageCode = (typeof ALL_LANGUAGES)[number]["code"];
/** What the configuration stores: a language, or "system" to follow the computer. */
export type LanguageSetting = "system" | LanguageCode;

const DICTIONARIES: Record<Exclude<LanguageCode, "en">, Record<string, string>> = {
  de,
  es,
  fr,
  pl,
  "pt-BR": ptBR,
  "zh-Hans": zhHans,
};

/**
 * The languages the window offers, in the order the menu shows them: English
 * and every language whose translation is there. A translation file that is
 * still empty keeps its language out of the menu.
 */
export const LANGUAGES = ALL_LANGUAGES.filter(
  (language) => language.code === "en" || Object.keys(DICTIONARIES[language.code]).length > 0,
);

let dictionary: Record<string, string> = {};

function isLanguage(code: string): code is LanguageCode {
  return LANGUAGES.some((language) => language.code === code);
}

/**
 * The language for a setting. "system" takes the first language of the
 * computer that the window has: "pl-PL" is Polish, every Portuguese is the
 * Brazilian one, every Chinese the simplified one. Anything else is English.
 */
export function resolveLanguage(
  setting: string | undefined,
  systemLanguages: readonly string[],
): LanguageCode {
  if (setting && isLanguage(setting)) return setting;
  for (const tag of systemLanguages) {
    const primary = tag.toLowerCase().split("-")[0];
    if (primary === "pt") return "pt-BR";
    if (primary === "zh") return "zh-Hans";
    if (isLanguage(primary)) return primary;
  }
  return "en";
}

/** Switches the window to the language of `setting` and returns its code. */
export function setLanguage(setting: string | undefined): LanguageCode {
  const code = resolveLanguage(setting, navigator.languages ?? [navigator.language]);
  dictionary = code === "en" ? {} : DICTIONARIES[code];
  document.documentElement.lang = code;
  return code;
}

/**
 * The text in the current language. `values` fill the `{name}` placeholders:
 * `t("{count} minutes added.", { count: 15 })`.
 */
export function t(source: string, values?: Record<string, string | number>): string {
  const text = dictionary[source] ?? source;
  if (!values) return text;
  return text.replace(/\{(\w+)\}/g, (placeholder, name: string) =>
    name in values ? String(values[name]) : placeholder,
  );
}

/**
 * Marks a text for translation without translating it yet, for constants
 * that are built before the language is known. Show it with `t(...)`.
 */
export function tn(source: string): string {
  return source;
}
