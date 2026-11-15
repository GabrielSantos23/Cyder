//! Bundled palettes, the override stylesheet and the JSON Custom Theme, ported from
//! `src/palettes.ts`, `src/themeTemplate.ts` and `src/customTheme.ts`.
//!
//! Everything [`build_theme_css`] returns ships to the page, comments included, so
//! a comment inside the template is a one-line section label for the DevTools
//! style pane and every explanation lives here in the Rust source instead.
//! `tests/theme_golden.rs` holds the output byte-identical to the TypeScript build.

use serde_json::{Map, Value};

/// The 12 semantic colour slots a palette fills for one colour scheme, as `#rrggbb`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchemeColours<S = &'static str> {
    /// Page background
    pub base: S,
    /// Opaque shelf, player and material backgrounds (darker than base)
    pub mantle: S,
    /// Footer background and deepest shadows (darker than mantle)
    pub crust: S,
    /// LCD fill, tracklist rows, banner dropdowns
    pub surface0: S,
    /// Scrollbar thumb, segmented control selection, generic accents
    pub surface1: S,
    /// Borders and dividers
    pub surface2: S,
    /// Tertiary text
    pub overlay: S,
    /// Primary text
    pub text: S,
    /// Secondary text
    pub subtext1: S,
    /// Emphasised secondary text
    pub subtext0: S,
    /// Key colour: brand, selection, scrubber fill, platter buttons
    pub accent: S,
    /// Accent rollover and pressed states
    pub accent_hover: S,
}

/// A theme: registry name, label, and one palette per colour scheme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeDefinition<S = &'static str> {
    pub name: S,
    pub label: S,
    pub dark: SchemeColours<S>,
    pub light: SchemeColours<S>,
}

/// Names of the themes Sidra ships, in the order the Style menu lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BundledThemeName {
    Catppuccin,
    Dracula,
    Everforest,
    Gruvbox,
    Nord,
    RosePine,
    Solarized,
    TokyoNight,
}

impl BundledThemeName {
    pub const ALL: [Self; 8] = [
        Self::Catppuccin,
        Self::Dracula,
        Self::Everforest,
        Self::Gruvbox,
        Self::Nord,
        Self::RosePine,
        Self::Solarized,
        Self::TokyoNight,
    ];

    /// The palette behind this name.
    pub fn theme(self) -> &'static ThemeDefinition {
        &BUNDLED_THEMES[self as usize]
    }

    pub fn as_str(self) -> &'static str {
        self.theme().name
    }
}

/// The active theme. `AppleMusic` means no override CSS is injected at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemeName {
    AppleMusic,
    Bundled(BundledThemeName),
    Custom,
}

impl ThemeName {
    /// Narrows a stored string to a theme Sidra can render.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "apple-music" => Some(Self::AppleMusic),
            "custom" => Some(Self::Custom),
            _ => BundledThemeName::ALL
                .into_iter()
                .find(|b| b.as_str() == value)
                .map(Self::Bundled),
        }
    }

    /// The name as stored in config.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AppleMusic => "apple-music",
            Self::Custom => "custom",
            Self::Bundled(b) => b.as_str(),
        }
    }

    /// The label shown in Settings.
    pub fn label(self) -> &'static str {
        match self {
            Self::AppleMusic => "Apple Music",
            Self::Custom => "Custom Theme",
            Self::Bundled(b) => b.theme().label,
        }
    }
}

// Upstream sources, all MIT: catppuccin/palette, dracula/dracula-theme,
// sainnhe/everforest, morhetz/gruvbox, nordtheme/nord, rose-pine/palette,
// altercation/solarized, tokyo-night/tokyo-night-vscode-theme.
// A "derived" shade has no upstream equivalent and was chosen to fit its slot.
//
// Invariants, checked in the tests below: overlay differs from subtext0 and crust
// from surface0, because each pair paints different elements; text and subtext0
// reach 4.5:1 against base after the template's alpha (Catppuccin Latte keeps
// its upstream 4.37:1 secondary).

macro_rules! scheme {
    ($base:literal $mantle:literal $crust:literal $s0:literal $s1:literal $s2:literal
     $overlay:literal $text:literal $sub1:literal $sub0:literal $accent:literal $hover:literal) => {
        SchemeColours {
            base: $base,
            mantle: $mantle,
            crust: $crust,
            surface0: $s0,
            surface1: $s1,
            surface2: $s2,
            overlay: $overlay,
            text: $text,
            subtext1: $sub1,
            subtext0: $sub0,
            accent: $accent,
            accent_hover: $hover,
        }
    };
}

// Dracula has no official light scheme; the dark palette serves both.
const DRACULA_DARK: SchemeColours = scheme!(
    "#282a36" "#21222c" "#191a21" "#343746" "#44475a" "#565966"
    "#6272a4" "#f8f8f2" "#d8d8d2" "#b8b8b2" "#bd93f9" "#ff79c6"
);

/// Every bundled theme, indexed by [`BundledThemeName`].
pub static BUNDLED_THEMES: [ThemeDefinition; 8] = [
    ThemeDefinition {
        name: "catppuccin",
        label: "Catppuccin",
        // Mocha / Latte
        dark: scheme!("#1e1e2e" "#181825" "#11111b" "#313244" "#45475a" "#585b70"
                      "#6c7086" "#cdd6f4" "#bac2de" "#a6adc8" "#f38ba8" "#eba0ac"),
        light: scheme!("#eff1f5" "#e6e9ef" "#dce0e8" "#ccd0da" "#bcc0cc" "#acb0be"
                       "#9ca0b0" "#4c4f69" "#5c5f77" "#6c6f85" "#d20f39" "#e64553"),
    },
    ThemeDefinition {
        name: "dracula",
        label: "Dracula",
        dark: DRACULA_DARK,
        light: DRACULA_DARK,
    },
    ThemeDefinition {
        name: "everforest",
        label: "Everforest",
        // Medium contrast; the light text is darkened to keep 4.5:1 after alpha.
        dark: scheme!("#2d353b" "#232a2e" "#1e2326" "#343f44" "#3d484d" "#475258"
                      "#859289" "#d3c6aa" "#d3c6aa" "#9da9a0" "#a7c080" "#83c092"),
        light: scheme!("#fdf6e3" "#f4f0d9" "#efebd4" "#e6e2cc" "#e0dcc7" "#bdc3af"
                       "#939f91" "#414b52" "#5c6a72" "#5c6a72" "#5d6b01" "#206e4e"),
    },
    ThemeDefinition {
        name: "gruvbox",
        label: "Gruvbox",
        // Dark crust #141617 is derived.
        dark: scheme!("#282828" "#1d2021" "#141617" "#3c3836" "#504945" "#665c54"
                      "#7c6f64" "#ebdbb2" "#d5c4a1" "#bdae93" "#fe8019" "#d65d0e"),
        light: scheme!("#fbf1c7" "#f2e5bc" "#ebdbb2" "#d5c4a1" "#bdae93" "#a89984"
                       "#928374" "#3c3836" "#504945" "#665c54" "#af3a03" "#d65d0e"),
    },
    ThemeDefinition {
        name: "nord",
        label: "Nord",
        // Polar Night with Frost accent; Snow Storm with a darker Frost accent.
        // Dark mantle, crust and overlay, and light surfaces and overlay, are derived.
        dark: scheme!("#2e3440" "#292e39" "#242933" "#3b4252" "#434c5e" "#4c566a"
                      "#616e88" "#eceff4" "#e5e9f0" "#d8dee9" "#88c0d0" "#81a1c1"),
        light: scheme!("#eceff4" "#e5e9f0" "#d8dee9" "#cdd4e0" "#c2c9d6" "#aab2c4"
                       "#7b88a1" "#2e3440" "#3b4252" "#4c566a" "#5e81ac" "#81a1c1"),
    },
    ThemeDefinition {
        name: "rose-pine",
        label: "Ros\u{e9} Pine",
        // Main / Dawn. Main mantle, crust and subtext1, and Dawn crust and subtext0, are derived.
        dark: scheme!("#191724" "#13111d" "#0e0c15" "#26233a" "#403d52" "#524f67"
                      "#6e6a86" "#e0def4" "#b8b5cf" "#908caa" "#ebbcba" "#eb6f92"),
        light: scheme!("#faf4ed" "#f2e9e1" "#ede0d4" "#f4ede8" "#dfdad9" "#cecacd"
                       "#9893a5" "#575279" "#797593" "#686486" "#d7827e" "#b4637a"),
    },
    ThemeDefinition {
        name: "solarized",
        label: "Solarized",
        // Dark mantle, crust and surface1, and light mantle and surfaces, are derived.
        dark: scheme!("#002b36" "#00252e" "#001f27" "#073642" "#0e4552" "#586e75"
                      "#657b83" "#eee8d5" "#839496" "#93a1a1" "#268bd2" "#2aa198"),
        light: scheme!("#fdf6e3" "#f5eed9" "#eee8d5" "#e6dfcb" "#ddd6c1" "#93a1a1"
                       "#839496" "#073642" "#657b83" "#586e75" "#268bd2" "#2aa198"),
    },
    ThemeDefinition {
        name: "tokyo-night",
        label: "Tokyo Night",
        // Night / Light
        dark: scheme!("#1a1b26" "#16161e" "#0f0f14" "#292e42" "#414868" "#565f89"
                      "#545c7e" "#c0caf5" "#a9b1d6" "#9aa5ce" "#7aa2f7" "#bb9af7"),
        light: scheme!("#e6e7ed" "#d6d8df" "#c1c2c7" "#dcdee3" "#bdc1cf" "#9da0ab"
                       "#707280" "#343b59" "#6c6e75" "#4a5272" "#2959aa" "#5a3e8e"),
    },
];

fn rgb(hex: &str) -> (u8, u8, u8) {
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    (byte(1), byte(3), byte(5))
}

fn triplet(hex: &str) -> String {
    let (r, g, b) = rgb(hex);
    format!("{r},{g},{b}")
}

fn rgba(hex: &str, alpha: f64) -> String {
    format!("rgba({},{alpha})", triplet(hex))
}

fn rgba_spaced(hex: &str, alpha: f64) -> String {
    let (r, g, b) = rgb(hex);
    format!("rgba({r}, {g}, {b}, {alpha})")
}

// Why the rules below exist (kept here because a comment in the output ships):
//
// The `*` block is the only declaration site for the accent group. The shadow DOM
// of amp-* elements does not inherit from :root, and Apple declares those
// variables without !important, so `*` wins at the root element too; it also
// carries variables Apple re-declares on the element itself. The sidebar reads
// --keyAccentColor rather than --keyColor, so both follow the palette.
//
// .chrome-player::before paints the player bar, which is itself transparent.
// Classical declares a gradient on that pseudo and music re-declares its fill
// there, so neither is reachable from the `*` block.
//
// .side-panel and its header wrapper paint a direct background-color.
//
// No ::-webkit-scrollbar-* rules: from Chrome 121 Chromium ignores them on any
// element with a non-auto scrollbar-color, which this block sets everywhere.
fn scheme_block<S: AsRef<str>>(c: &SchemeColours<S>) -> String {
    let (base, mantle, crust) = (c.base.as_ref(), c.mantle.as_ref(), c.crust.as_ref());
    let (s0, s1, s2) = (
        c.surface0.as_ref(),
        c.surface1.as_ref(),
        c.surface2.as_ref(),
    );
    let (overlay, text, sub1, sub0) = (
        c.overlay.as_ref(),
        c.text.as_ref(),
        c.subtext1.as_ref(),
        c.subtext0.as_ref(),
    );
    let (accent, hover) = (c.accent.as_ref(), c.accent_hover.as_ref());
    let player_bg = rgba(mantle, 0.88);
    let text85 = rgba(text, 0.85);
    format!(
        r#"  :root {{
    /* Background & Structure */
    --pageBG: {base} !important;
    --pageBG-rgb: {base_rgb} !important;
    --opaqueShelfBG: {mantle} !important;
    --fallbackMaterialBG: {mantle97} !important;
    --shelfBG: {s0_15} !important;
    --genericJoeColor: {s1} !important;

    /* Text */
    --systemPrimary: {text85} !important;
    --systemPrimary-vibrant: {text} !important;
    --systemSecondary: {sub1_55} !important;
    --systemSecondary-vibrant: {sub0} !important;
    --systemTertiary: {overlay_25} !important;
    --systemQuaternary: {s1_15} !important;
    --systemQuinary: {s0_08} !important;

    /* Borders & Dividers */
    --labelDivider: {s2_2} !important;
    --vibrantDivider: {s2_25} !important;

    /* Player */
    --playerBackground: {player_bg} !important;
    --playerBackgroundFallback: {mantle97} !important;
    --playerLCDBGFill: {s0} !important;
    --playerMissingArtworkBg: {s1} !important;
    --playerMissingArtworkIcon: {s2} !important;
    --playerScrubberFill: {accent} !important;
    --playerScrubberTrack: {s1_2} !important;
    --playerPlatterButtonBGFill: {accent} !important;
    --playerPlatterButtonIconFill: {base} !important;
    --playerDropShadow2: {crust_1} !important;

    /* Navigation */
    --segmentedControlBG: {s0_2} !important;
    --segmentedControlSelectedBG: {s1} !important;

    /* Tracklist */
    --tracklistHoverColor: {s0_08} !important;
    --tracklistAltRowColor: {s0_03} !important;

    /* Materials */
    --systemStandardThickMaterialSover: {mantle72} !important;
    --systemHeaderMaterialSover: {mantle8} !important;
    --systemToolbarTitlebarMaterialSover: {mantle8} !important;
  }}

  /* Scrollbars */
  * {{
    scrollbar-color: {s1} {mantle} !important;
  }}

  /* Accent variables, and variables Apple re-declares on the element */
  * {{
    --keyColor: {accent} !important;
    --keyAccentColor: {accent} !important;
    --keyColor-rgb: {accent_rgb} !important;
    --keyColor-rollover: {hover} !important;
    --keyColor-pressed: {hover} !important;
    --keyColor-deepPressed: {hover} !important;
    --keyColor-disabled: {accent35} !important;
    --musicKeyColor: {accent} !important;
    --musicBrandBG: {accent} !important;
    --selectionColor: {accent} !important;
    --chromePlayerBGFill: {player_bg} !important;
    --lcd-bg-color: {s0} !important;
  }}

  /* Player bar */
  .chrome-player::before {{
    background-color: {player_bg} !important;
    background-image: none !important;
  }}

  /* Side panels (Lyrics + Up Next) */
  .side-panel {{
    background-color: {mantle97_spaced} !important;
    backdrop-filter: blur(50px) saturate(100%) !important;
  }}
  .side-panel.side-panel-header-wrapper,
  .side-panel .side-panel-header-wrapper,
  .side-panel-header-wrapper {{
    background-color: {mantle97_spaced} !important;
  }}

  /* Footer */
  :is(
    footer,
    .scrollable-page footer
  ) {{
    background: {crust} !important;
    background-color: {crust} !important;
    border-color: {s2_2} !important;
    color: {text85} !important;
  }}

  :is(
    footer,
    .scrollable-page footer
  ) :is(a, button, select, [role="button"], [class*="dropdown"]) {{
    background-color: {mantle} !important;
    border-color: {s2_25} !important;
    color: {text85} !important;
  }}

  /* Country/region picker modal background */
  [class*="locale-switcher"] {{
    background: {mantle} !important;
    background-color: {mantle} !important;
  }}

  /* Country/region banner (bottom strip) */
  [data-testid="banner-container"] {{
    background: {mantle} !important;
    background-color: {mantle} !important;
    color: {text85} !important;
    border-color: {s2_25} !important;
  }}
  [data-testid="banner-container"] [data-testid="close-button"] {{
    color: {text85} !important;
    fill: {text85} !important;
  }}
  [data-testid="banner-container"] [data-testid="close-button"] svg path {{
    fill: {text85} !important;
  }}
  [data-testid="banner-container"] [data-testid="dropdown-button"] {{
    background-color: {s0} !important;
    border-color: {s2_4} !important;
    color: {text85} !important;
  }}"#,
        base_rgb = triplet(base),
        mantle97 = rgba(mantle, 0.97),
        mantle97_spaced = rgba_spaced(mantle, 0.97),
        mantle72 = rgba(mantle, 0.72),
        mantle8 = rgba(mantle, 0.8),
        s0_15 = rgba(s0, 0.15),
        s0_08 = rgba(s0, 0.08),
        s0_03 = rgba(s0, 0.03),
        s0_2 = rgba(s0, 0.2),
        s1_15 = rgba(s1, 0.15),
        s1_2 = rgba(s1, 0.2),
        s2_2 = rgba(s2, 0.2),
        s2_25 = rgba(s2, 0.25),
        s2_4 = rgba(s2, 0.4),
        sub1_55 = rgba(sub1, 0.55),
        overlay_25 = rgba(overlay, 0.25),
        crust_1 = rgba(crust, 0.1),
        accent_rgb = triplet(accent),
        accent35 = rgba(accent, 0.35),
    )
}

/// The whole override stylesheet for a theme, both colour schemes included.
///
/// Apple hardcodes `background-color: rgb(214, 0, 23)` on
/// `.button.primary button.click-action` and never reads `--keyColor` there,
/// so accent-coloured buttons need the direct override at the end.
pub fn build_theme_css<S: AsRef<str>>(theme: &ThemeDefinition<S>) -> String {
    format!(
        "/*\n * {label} theme for Apple Music\n * Dark and light variants via prefers-color-scheme\n * Generated by src/themeTemplate.ts from src/palettes.ts\n */\n\n\
@media (prefers-color-scheme: dark) {{\n{dark}\n}}\n\n\
@media (prefers-color-scheme: light) {{\n{light}\n}}\n\n\
/* Accent-coloured buttons */\n.button.primary button.click-action {{\n  background-color: var(--keyColor) !important;\n}}\n",
        label = theme.label.as_ref(),
        dark = scheme_block(&theme.dark),
        light = scheme_block(&theme.light),
    )
}

fn is_hex_colour(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn parse_scheme(value: &Value) -> Option<SchemeColours<String>> {
    let obj = value.as_object()?;
    let slot = |name: &str| {
        obj.get(name)
            .and_then(Value::as_str)
            .filter(|s| is_hex_colour(s))
            .map(str::to_owned)
    };
    Some(SchemeColours {
        base: slot("base")?,
        mantle: slot("mantle")?,
        crust: slot("crust")?,
        surface0: slot("surface0")?,
        surface1: slot("surface1")?,
        surface2: slot("surface2")?,
        overlay: slot("overlay")?,
        text: slot("text")?,
        subtext1: slot("subtext1")?,
        subtext0: slot("subtext0")?,
        accent: slot("accent")?,
        accent_hover: slot("accentHover")?,
    })
}

/// Parses `custom-theme.json`: all 12 slots as `#RRGGBB` under `dark`, and under
/// `light` when present (an omitted `light` copies `dark`). Unknown keys are
/// ignored and only known slots are copied; raw CSS is never accepted.
pub fn parse_custom_theme(json: &str) -> Option<ThemeDefinition<String>> {
    let value: Map<String, Value> = serde_json::from_str::<Value>(json)
        .ok()?
        .as_object()?
        .clone();
    let dark = parse_scheme(value.get("dark")?)?;
    let light = match value.get("light") {
        Some(l) => parse_scheme(l)?,
        None => dark.clone(),
    };
    Some(ThemeDefinition {
        name: "custom".into(),
        label: "Custom Theme".into(),
        dark,
        light,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channels_luminance(r: f64, g: f64, b: f64) -> f64 {
        let lin = |c: f64| {
            let s = c / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    fn luminance(hex: &str) -> f64 {
        let (r, g, b) = rgb(hex);
        channels_luminance(r.into(), g.into(), b.into())
    }

    // Unrounded blend, as the TypeScript test computes it.
    fn composite_luminance(fg: &str, alpha: f64, bg: &str) -> f64 {
        let (f, b) = (rgb(fg), rgb(bg));
        let mix = |x: u8, y: u8| f64::from(x) * alpha + f64::from(y) * (1.0 - alpha);
        channels_luminance(mix(f.0, b.0), mix(f.1, b.1), mix(f.2, b.2))
    }

    /// WCAG 2.x ratio rounded to two decimals, which is what the floors are written in.
    fn ratio(a: f64, b: f64) -> f64 {
        ((a.max(b) + 0.05) / (a.min(b) + 0.05) * 100.0).round() / 100.0
    }

    fn schemes() -> impl Iterator<Item = (&'static str, &'static str, &'static SchemeColours)> {
        BUNDLED_THEMES
            .iter()
            .flat_map(|t| [(t.name, "dark", &t.dark), (t.name, "light", &t.light)])
    }

    #[test]
    fn names_labels_and_slots_are_well_formed() {
        for (i, b) in BundledThemeName::ALL.into_iter().enumerate() {
            assert!(std::ptr::eq(b.theme(), &BUNDLED_THEMES[i]));
            assert!(b
                .as_str()
                .split('-')
                .all(|w| !w.is_empty() && w.bytes().all(|c| c.is_ascii_lowercase())));
            assert_eq!(ThemeName::parse(b.as_str()), Some(ThemeName::Bundled(b)));
        }
        let mut labels: Vec<_> = BUNDLED_THEMES.iter().map(|t| t.label).collect();
        labels.dedup();
        assert_eq!(labels.len(), BUNDLED_THEMES.len());
        for (name, scheme, c) in schemes() {
            for v in [
                c.base,
                c.mantle,
                c.crust,
                c.surface0,
                c.surface1,
                c.surface2,
                c.overlay,
                c.text,
                c.subtext1,
                c.subtext0,
                c.accent,
                c.accent_hover,
            ] {
                assert!(
                    is_hex_colour(v) && v == v.to_lowercase(),
                    "{name} {scheme} {v}"
                );
            }
        }
    }

    #[test]
    fn theme_names_parse_and_label() {
        assert_eq!(ThemeName::parse("apple-music"), Some(ThemeName::AppleMusic));
        assert_eq!(ThemeName::parse("custom"), Some(ThemeName::Custom));
        assert_eq!(ThemeName::parse("solarised"), None);
        assert_eq!(
            ThemeName::Bundled(BundledThemeName::RosePine).label(),
            "Ros\u{e9} Pine"
        );
        assert_eq!(ThemeName::Custom.label(), "Custom Theme");
        assert_eq!(ThemeName::AppleMusic.label(), "Apple Music");
    }

    #[test]
    fn paired_slots_differ() {
        for (name, scheme, c) in schemes() {
            assert_ne!(c.overlay, c.subtext0, "{name} {scheme}: overlay = subtext0");
            assert_ne!(c.crust, c.surface0, "{name} {scheme}: crust = surface0");
        }
    }

    #[test]
    fn text_meets_the_contrast_floor() {
        for (name, scheme, c) in schemes() {
            let bg = luminance(c.base);
            let primary = ratio(composite_luminance(c.text, 0.85, c.base), bg);
            assert!(primary >= 4.5, "{name} {scheme}: primary {primary:.2}:1");
            let floor = if (name, scheme) == ("catppuccin", "light") {
                4.37
            } else {
                4.5
            };
            let secondary = ratio(luminance(c.subtext0), bg);
            assert!(
                secondary >= floor,
                "{name} {scheme}: secondary {secondary:.2}:1"
            );
        }
    }

    #[test]
    fn output_comments_are_one_line_labels_past_the_banner() {
        for t in &BUNDLED_THEMES {
            let css = build_theme_css(t);
            let body = &css[css.find("*/").unwrap() + 2..];
            for comment in body.split("/*").skip(1) {
                let text = &comment[..comment.find("*/").unwrap()];
                assert!(
                    !text.contains('\n'),
                    "{}: multi-line comment {text:?}",
                    t.name
                );
            }
            assert!(!css.contains("::-webkit-scrollbar"));
        }
    }

    const FULL: &str = r##"{"base":"#000000","mantle":"#111111","crust":"#222222","surface0":"#333333","surface1":"#444444","surface2":"#555555","overlay":"#666666","text":"#FFFFFF","subtext1":"#eeeeee","subtext0":"#dddddd","accent":"#ff0000","accentHover":"#00ff00"}"##;

    #[test]
    fn custom_theme_copies_dark_into_missing_light() {
        let theme = parse_custom_theme(&format!(r#"{{"dark":{FULL}}}"#)).unwrap();
        assert_eq!(theme.light, theme.dark);
        assert_eq!(theme.dark.text, "#FFFFFF");
        assert_eq!(theme.label, "Custom Theme");
        let css = build_theme_css(&theme);
        assert!(css.contains("--pageBG: #000000 !important;"));
    }

    #[test]
    fn custom_theme_refuses_incomplete_or_invalid_input() {
        let missing = FULL.replace(r##","accentHover":"#00ff00""##, "");
        let short_hex = FULL.replace("#ff0000", "#f00");
        let not_hex = FULL.replace("#ff0000", "#gg0000");
        let css = FULL.replace("#ff0000", "red;} body{x:y");
        for json in [
            "not json".to_owned(),
            "[]".to_owned(),
            "{}".to_owned(),
            format!(r#"{{"light":{FULL}}}"#),
            format!(r#"{{"dark":{missing}}}"#),
            format!(r#"{{"dark":{short_hex}}}"#),
            format!(r#"{{"dark":{not_hex}}}"#),
            format!(r#"{{"dark":{css}}}"#),
            format!(r#"{{"dark":{FULL},"light":{missing}}}"#),
            format!(r#"{{"dark":{FULL},"light":null}}"#),
        ] {
            assert_eq!(parse_custom_theme(&json), None, "{json}");
        }
    }
}
