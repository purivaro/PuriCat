import { create } from "zustand";

import i18n, { isLanguage, type Language } from "../i18n";
import { ipc } from "../lib/ipc";

export type ThemeChoice = "light" | "dark" | "system";

interface UiSettings {
  theme: ThemeChoice;
  language: Language;
  explorerOpen: boolean;
  aiOpen: boolean;
  explorerWidth: number;
  aiWidth: number;
  /** Look for a newer PuriCat at start-up (one request to GitHub). */
  autoUpdate: boolean;
}

interface SettingsState extends UiSettings {
  loaded: boolean;
  load: () => Promise<void>;
  update: (patch: Partial<UiSettings>) => void;
}

const DEFAULTS: UiSettings = {
  theme: "system",
  language: "en",
  explorerOpen: true,
  aiOpen: false,
  explorerWidth: 268,
  aiWidth: 340,
  autoUpdate: true,
};

/** Mirror of the theme for the very first paint, before the settings store answers. */
const THEME_HINT = "puricat.theme";

const media = window.matchMedia("(prefers-color-scheme: dark)");

export function resolveTheme(choice: ThemeChoice): "light" | "dark" {
  return choice === "system" ? (media.matches ? "dark" : "light") : choice;
}

function applyTheme(choice: ThemeChoice) {
  document.documentElement.dataset.theme = resolveTheme(choice);
  try {
    localStorage.setItem(THEME_HINT, choice);
  } catch {
    /* private mode: the hint is only an optimisation */
  }
}

function applyLanguage(language: Language) {
  document.documentElement.lang = language;
  void i18n.changeLanguage(language);
}

export function bootstrapTheme() {
  let hint: string | null = null;
  try {
    hint = localStorage.getItem(THEME_HINT);
  } catch {
    /* ignore */
  }
  applyTheme(hint === "light" || hint === "dark" ? hint : "system");
}

function clamp(n: unknown, min: number, max: number, fallback: number): number {
  return typeof n === "number" && Number.isFinite(n) ? Math.min(max, Math.max(min, n)) : fallback;
}

function pick(s: UiSettings): UiSettings {
  const { theme, language, explorerOpen, aiOpen, explorerWidth, aiWidth, autoUpdate } = s;
  return { theme, language, explorerOpen, aiOpen, explorerWidth, aiWidth, autoUpdate };
}

export const useSettings = create<SettingsState>((set, get) => ({
  ...DEFAULTS,
  loaded: false,

  load: async () => {
    const all = await ipc.getSettings();
    let saved: Partial<UiSettings> = {};
    try {
      saved = all.ui ? (JSON.parse(all.ui) as Partial<UiSettings>) : {};
    } catch {
      /* corrupt value: fall back to defaults */
    }
    const next: UiSettings = {
      theme: saved.theme === "light" || saved.theme === "dark" ? saved.theme : "system",
      // First run: follow the OS language when we have a translation for it.
      language: isLanguage(saved.language)
        ? saved.language
        : navigator.language.toLowerCase().startsWith("th")
          ? "th"
          : "en",
      explorerOpen: saved.explorerOpen ?? DEFAULTS.explorerOpen,
      aiOpen: saved.aiOpen ?? DEFAULTS.aiOpen,
      explorerWidth: clamp(saved.explorerWidth, 200, 480, DEFAULTS.explorerWidth),
      aiWidth: clamp(saved.aiWidth, 280, 560, DEFAULTS.aiWidth),
      autoUpdate: saved.autoUpdate ?? DEFAULTS.autoUpdate,
    };
    applyTheme(next.theme);
    applyLanguage(next.language);
    set({ ...next, loaded: true });
  },

  update: (patch) => {
    set(patch);
    const next = pick(get());
    if (patch.theme) applyTheme(next.theme);
    if (patch.language) applyLanguage(next.language);
    void ipc.setSetting("ui", JSON.stringify(next));
  },
}));

media.addEventListener("change", () => {
  const { theme } = useSettings.getState();
  if (theme === "system") applyTheme("system");
});
