import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./en.json";
import th from "./th.json";

export const LANGUAGES = [
  { code: "en", label: "English" },
  { code: "th", label: "ไทย" },
] as const;

export type Language = (typeof LANGUAGES)[number]["code"];

export function isLanguage(v: unknown): v is Language {
  return LANGUAGES.some((l) => l.code === v);
}

void i18n.use(initReactI18next).init({
  resources: { en: { translation: en }, th: { translation: th } },
  lng: "en",
  fallbackLng: "en",
  interpolation: { escapeValue: false }, // React escapes for us
});

export default i18n;
