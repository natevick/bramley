# Theming

Bramley picks its colours in this order:

1. `--theme dark|light|system|omarchy` (dev builds only) overrides everything.
2. Otherwise, if `$XDG_STATE_HOME/omarchy/current/theme/colors.toml` exists (default `~/.local/state/...`), Omarchy's theme.
3. Otherwise the system light or dark setting. macOS and Windows have no such file, so they always land here.

## Omarchy mapping

| colors.toml | Bramley |
|---|---|
| `mode` | dark or light |
| `background` | panels, queue sheet, menus |
| `dark_background` | sidebar and compact drawer |
| `darker_background` | text inputs, scroll tracks |
| `lighter_background` | button fill |
| `selection` | hover and open fill |
| `accent` | selected rows, toggles, slider fill, pressed buttons |
| `muted` | borders and separators |
| `foreground` / `bright_foreground` / `dark_foreground` | text / strong text / weak text |
| `red` | banners and notices |

Text colours are nudged toward black or white until they clear WCAG AA against the surfaces they sit on: 4.5:1 for text and strong text, 3:1 for weak text. A palette with a faint `dark_foreground` stays readable.

## Live switching

Bramley re-reads the roughly 600-byte file about once a second, so `omarchy-theme-set` shows up within about 2 seconds. While the directory is missing mid-swap the current theme stays. After 3 seconds of absence, or on a malformed file, Bramley uses the normal theme and logs once, without the path.

Unknown keys are ignored. Every line must be `key = "value"`, a comment or blank; anything else counts as malformed.

## Themes without a colors.toml

Omarchy generates a colors.toml with `omarchy-theme-colors-from-alacritty` for a theme that ships none. That file has only `accent`, `selection`, `background`, `foreground` and the numbered colours, so Bramley treats it as malformed: it uses the system palette and logs "omarchy colors.toml is unreadable". All 22 themes installed on Nate's machine ship a full colors.toml (checked 2026-10-21).

## XDG_STATE_HOME

`omarchy-theme-set` always writes `$HOME/.local/state/omarchy/current/theme` and ignores `XDG_STATE_HOME`. Bramley honours an absolute `XDG_STATE_HOME`, so with one set Bramley looks in `$XDG_STATE_HOME/omarchy`, finds no theme there, and uses the system palette.
