# 0011 — i18n with i18next, CSS-variable design tokens, bundled fonts

## Status

Accepted. Implemented (English and Thai; light, dark and system themes).

## Context

The brief sets a white/orange light theme, a coordinated dark theme, and English plus Thai from the start. A public desktop app should not fetch fonts or other assets from the network.

## Decision

- **Strings:** i18next 27 with react-i18next 18; one JSON catalog per language (`src/i18n/en.json`, `th.json`). English is the fallback. The first run follows the OS language when a catalog exists; switching needs no restart.
- **Tokens:** colours, shadows and fonts are CSS custom properties on `:root`, overridden under `[data-theme="dark"]`, and exposed to Tailwind 4 as theme colours. Components use token names, not hex values.
- **Theme choice:** light, dark or system, stored in the settings store. A `localStorage` hint applies the theme before the store answers, to avoid a flash.
- **Fonts:** Inter, Noto Sans Thai and JetBrains Mono as variable fonts from `@fontsource-variable/*`, bundled with the app.
- **Motion:** a `prefers-reduced-motion` rule disables animations.

## Alternatives considered

| Option | Why not |
|---|---|
| FormatJS / react-intl | ICU messages are stronger for plurals; i18next is sufficient for the current strings |
| A theming library or CSS-in-JS | Custom properties are enough and cost nothing at runtime |
| System fonts only | Thai rendering and metrics would vary by OS version |
| Web fonts from a CDN | Network dependency and a privacy leak |

## Consequences

- Backend error messages (from `puricat-core` and the database drivers) are English only and are shown as-is.
- Nothing checks that the Thai catalog has every key; a missing key falls back to English silently.
- Number and date formatting use `Intl` in the UI; values from the database are shown exactly as the server renders them and are not localised.

## Open accessibility item

White text on the brand orange `#FF7A18` has a contrast ratio of about 2.6:1, below WCAG AA for small text. The palette is the product owner's confirmed choice, so the current mitigation is:

- primary buttons use semibold text;
- orange text on white uses a deeper token, `--accent-text` (`#C2500A`);
- the dark theme uses dark text on its orange fill.

A full contrast and accessibility audit is a Phase 7 task. Until then no WCAG conformance is claimed.

## Licensing

i18next and react-i18next MIT. Inter, Noto Sans Thai and JetBrains Mono are OFL-1.1; the licence text must ship with the app ([LICENSES.md](../../LICENSES.md)).
