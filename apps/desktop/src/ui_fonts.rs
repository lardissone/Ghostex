use gpui::{App, EXACT_FONT_FAMILY_PREFIX, SharedString};
use std::sync::{Arc, Mutex};

/// CDXC:DesignSystem 2026-09-26 DECISION:
/// User: Windows should render with the same quality, type sizes and spacing as macOS.
pub(crate) const UI_FONT: &str = "Inter";

/// The Files view's type sizes: its editor text, its line numbers and its file list names. The
/// Changes view draws its diff with the same three, so both views read at one size.
pub(crate) const FILES_TEXT_SIZE: f32 = 14.0;
pub(crate) const FILES_LINE_NUMBER_SIZE: f32 = 12.0;
/// `.manage-file-name`: 15.55px at weight 300.
pub(crate) const FILES_NAME_SIZE: f32 = 15.55;

/// CDXC:DesignSystem 2026-09-26 WHY:
/// The bundled static faces identify as Inter, not Inter Variable. Register them before the first window so chrome and Docs use the same metrics without relying on fonts installed on the host.
pub(crate) fn register(cx: &App) {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| {
        let fonts = vec![
            include_bytes!("../../../.dependencies/app-fonts/inter/Inter-Light.ttf")
                .as_slice()
                .into(),
            include_bytes!("../../../.dependencies/app-fonts/inter/Inter-Regular.ttf")
                .as_slice()
                .into(),
            include_bytes!("../../../.dependencies/app-fonts/inter/Inter-Medium.ttf")
                .as_slice()
                .into(),
            include_bytes!("../../../.dependencies/app-fonts/inter/Inter-SemiBold.ttf")
                .as_slice()
                .into(),
            include_bytes!("../../../.dependencies/app-fonts/inter/Inter-Bold.ttf")
                .as_slice()
                .into(),
            include_bytes!("../../../.dependencies/app-fonts/inter/Inter-Italic.ttf")
                .as_slice()
                .into(),
        ];
        if let Err(error) = cx.text_system().add_fonts(fonts) {
            eprintln!("[ghostex-gpui] Could not register UI fonts: {error}");
        }
    });
}

/// The setting that names the app font. It keeps its older name, from when it only reached the
/// chat, so a font someone already saved stays saved.
pub(crate) const APP_FONT_SETTING: &str = "sessionChatFontFamily";

/// Every family the app asks for when it draws ordinary, non-monospace text, its implicit default
/// included. Each surface keeps its own look (Inter in the chrome, DM Sans in the chat, the system
/// face in dialogs) until an app font is picked, and then all of them draw in it.
///
/// CDXC:DesignSystem 2026-10-31 DECISION:
/// User: "add list of the fonts so person can pick one of the installed fonts for the app" and "Font should apply everywhere (except for monospace, monospace should remain as is font)". The pick reaches every surface through one substitution in gpui's text system (`TextSystem::set_font_family_substitution`), not through the call sites: text with no family of its own inherits gpui's default, and measuring asks for the same fonts as drawing, so a list of call sites would keep missing some. No pick installs no substitution, so the default look is untouched. A new proportional family goes in this list, and `every_named_family_is_classified` fails until it does.
/// SEE-ALSO: .dependencies/zed/crates/gpui/src/text_system.rs.
pub(crate) const PROPORTIONAL_FAMILIES: &[&str] = &[
    ".SystemUIFont",
    ".AppleSystemUIFont",
    ".ZedSans",
    "Zed Plex Sans",
    "IBM Plex Sans",
    "Segoe UI",
    UI_FONT,
    "DM Sans",
    "DM Sans 14pt",
    "Manrope",
    "Arial",
    "Helvetica",
];

/// The monospace families the app asks for by name, which an app font never replaces.
#[cfg(test)]
const MONOSPACE_FAMILIES: &[&str] = &[
    ".AppleSystemUIFontMonospaced",
    ".ZedMono",
    "Consolas",
    "IBM Plex Mono",
    "IBM Plex Mono Medium",
    "JetBrains Mono",
    "JetBrainsMono Nerd Font",
    "Lilex",
    "Menlo",
    "SF Mono",
    "monospace",
];

/// The family `value` of the app font setting names, if it names one: blank is the app's default,
/// and a CSS stack from an older save counts as its first entry.
pub(crate) fn picked_family(value: &str) -> Option<&str> {
    let family = value
        .split(',')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches(|character| character == '"' || character == '\'');
    match family {
        "" | "system-ui" | "ui-sans-serif" | "sans-serif" | "-apple-system" => None,
        family => Some(family),
    }
}

/// Applies the saved app font to the whole process, once for each change.
pub(crate) fn apply_app_font(settings: &serde_json::Map<String, serde_json::Value>, cx: &mut App) {
    static APPLIED: Mutex<Option<AppliedFont>> = Mutex::new(None);
    let picked = settings
        .get(APP_FONT_SETTING)
        .and_then(serde_json::Value::as_str)
        .and_then(picked_family);
    let mut applied = APPLIED.lock().unwrap_or_else(|poison| poison.into_inner());
    let theme = gpui_component::Theme::global(cx).font_family.clone();
    let state = applied.get_or_insert_with(|| AppliedFont {
        family: None,
        theme_family: theme,
    });
    if state.family.as_deref() == picked {
        return;
    }
    state.family = picked.map(str::to_string);
    let theme_family = state.theme_family.clone();
    drop(applied);
    let to = picked.map(|family| SharedString::from(family.to_string()));
    let from = PROPORTIONAL_FAMILIES
        .iter()
        .map(|family| SharedString::new_static(family))
        .chain([theme_family.clone()]);
    cx.text_system().set_font_family_substitution(from, to);
    // gpui-component re-resolves `.SystemUIFont` when the appearance changes and can name the
    // picked family on its theme, which must not outlive the pick.
    gpui_component::Theme::global_mut(cx).font_family = theme_family;
    cx.refresh_windows();
}

struct AppliedFont {
    family: Option<String>,
    /// gpui-component's own UI family, resolved once before any pick was applied. On Linux it is
    /// whichever installed family `.SystemUIFont` landed on.
    theme_family: SharedString,
}

/// A family name that draws in exactly that family whatever the app font is: how a row of the
/// font list shows its own face.
pub(crate) fn exact_family(name: &str) -> SharedString {
    format!("{EXACT_FONT_FAMILY_PREFIX}{name}").into()
}

/// Whether a family looks monospaced from its name. Asking the fonts themselves would load every
/// installed font; the list only uses this to sort, so a miss costs one row's position.
pub(crate) fn looks_monospaced(name: &str) -> bool {
    let lower = name.to_lowercase();
    let words = lower.split([' ', '-', '_']).collect::<Vec<_>>();
    [
        "mono",
        "monospace",
        "monospaced",
        "code",
        "terminal",
        "fixed",
        "typewriter",
    ]
    .iter()
    .any(|word| words.contains(word))
        || lower.ends_with("mono")
        || lower.ends_with("code")
        || [
            "andale",
            "consolas",
            "courier",
            "inconsolata",
            "menlo",
            "monaco",
            "sfmono",
            "lilex",
            "iosevka",
            "fira code",
            "cascadia",
        ]
        .iter()
        .any(|name| lower.contains(name))
}

/// Whether a family draws its own name legibly. Symbol fonts would show a row of pictures.
pub(crate) fn draws_its_own_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    ![
        "dingbats",
        "wingdings",
        "webdings",
        "symbol",
        "ornaments",
        "emoji",
        "icons",
        "glyphs",
    ]
    .iter()
    .any(|word| lower.contains(word))
}

/// One row of the font list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FontRow {
    /// The app's own fonts; always first, so it is easy to get back to.
    AppDefault,
    /// The saved family when it is not on this computer. It stays saved and listed, and the app
    /// draws in its default fonts meanwhile.
    NotInstalled(String),
    Family {
        name: String,
        monospaced: bool,
    },
}

impl FontRow {
    /// What choosing this row saves.
    pub(crate) fn value(&self) -> &str {
        match self {
            FontRow::AppDefault => "",
            FontRow::NotInstalled(name) | FontRow::Family { name, .. } => name,
        }
    }
}

/// The families `names` a person can pick from: hidden faces out, ordinary fonts first and the
/// monospaced ones after them, each group alphabetical.
pub(crate) fn pickable_families(names: Vec<String>) -> Vec<(String, bool)> {
    let mut families: Vec<(String, bool)> = names
        .into_iter()
        .filter(|name| !name.is_empty() && !name.starts_with(['.', '@']))
        .map(|name| {
            let monospaced = looks_monospaced(&name);
            (name, monospaced)
        })
        .collect();
    families.sort_by_cached_key(|(name, monospaced)| (*monospaced, name.to_lowercase()));
    families.dedup_by(|left, right| left.0 == right.0);
    families
}

/// The rows the list shows for what is typed in its search field. `installed` is `None` until the
/// fonts have been read, so a saved family is not called missing before anyone has looked.
pub(crate) fn font_rows(
    installed: Option<&[(String, bool)]>,
    saved: &str,
    query: &str,
) -> Vec<FontRow> {
    let query = query.trim().to_lowercase();
    let matches = |name: &str| query.is_empty() || name.to_lowercase().contains(&query);
    let mut rows = Vec::new();
    if matches("App default") {
        rows.push(FontRow::AppDefault);
    }
    if let (Some(installed), Some(saved)) = (installed, picked_family(saved))
        && matches(saved)
        && !installed
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(saved))
    {
        rows.push(FontRow::NotInstalled(saved.to_string()));
    }
    rows.extend(
        installed
            .unwrap_or_default()
            .iter()
            .filter(|(name, _)| matches(name))
            .map(|(name, monospaced)| FontRow::Family {
                name: name.clone(),
                monospaced: *monospaced,
            }),
    );
    rows
}

static INSTALLED_FAMILIES: Mutex<Option<Arc<Vec<(String, bool)>>>> = Mutex::new(None);

/// The installed families from the last read, once one has finished.
pub(crate) fn installed_families() -> Option<Arc<Vec<(String, bool)>>> {
    INSTALLED_FAMILIES
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
}

/// Reads the installed families off the UI thread: asking the platform walks every font on the
/// computer. Runs at launch so the first open of Settings has them, and again when the list opens
/// so a font installed since then appears. The returned task resolves when the read is stored.
pub(crate) fn read_installed_families(cx: &App) -> gpui::Task<()> {
    let text_system = cx.text_system().clone();
    cx.background_executor().spawn(async move {
        let families = Arc::new(pickable_families(text_system.all_font_names()));
        *INSTALLED_FAMILIES
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(families);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn families(names: &[&str]) -> Vec<(String, bool)> {
        pickable_families(names.iter().map(|name| name.to_string()).collect())
    }

    #[test]
    fn blank_and_generic_names_are_the_app_default() {
        for value in ["", "   ", "system-ui", "sans-serif", "-apple-system, Arial"] {
            assert_eq!(picked_family(value), None, "{value:?}");
        }
    }

    #[test]
    fn a_saved_name_is_trimmed_and_a_stack_counts_as_its_first_entry() {
        assert_eq!(picked_family("  Georgia "), Some("Georgia"));
        assert_eq!(picked_family("'Fira Sans', sans-serif"), Some("Fira Sans"));
    }

    #[test]
    fn the_app_default_comes_first() {
        let installed = families(&["Zapfino", "Arial"]);
        let rows = font_rows(Some(&installed), "Arial", "");
        assert_eq!(rows[0], FontRow::AppDefault);
        assert_eq!(rows[0].value(), "");
        assert_eq!(rows[1].value(), "Arial");
    }

    #[test]
    fn a_saved_font_that_is_not_installed_stays_listed() {
        let installed = families(&["Arial"]);
        let rows = font_rows(Some(&installed), "Gone Font", "");
        assert_eq!(rows[1], FontRow::NotInstalled("Gone Font".to_string()));
        assert_eq!(rows[1].value(), "Gone Font");
    }

    #[test]
    fn a_saved_font_is_not_called_missing_before_the_fonts_are_read() {
        assert_eq!(font_rows(None, "Arial", ""), vec![FontRow::AppDefault]);
    }

    #[test]
    fn a_saved_font_that_is_installed_is_listed_once() {
        let installed = families(&["Arial"]);
        let rows = font_rows(Some(&installed), "arial", "");
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[1], FontRow::Family { .. }));
    }

    #[test]
    fn hidden_faces_are_left_out_and_monospaced_fonts_come_last() {
        let names: Vec<String> = families(&[
            "Menlo",
            ".SF NS",
            "@MS Gothic",
            "zapfino",
            "Arial",
            "JetBrains Mono",
        ])
        .into_iter()
        .map(|(name, _)| name)
        .collect();
        assert_eq!(names, ["Arial", "zapfino", "JetBrains Mono", "Menlo"]);
    }

    #[test]
    fn monospaced_fonts_stay_pickable_and_searchable() {
        let installed = families(&["Arial", "Menlo"]);
        let rows = font_rows(Some(&installed), "", "men");
        assert_eq!(
            rows,
            vec![FontRow::Family {
                name: "Menlo".to_string(),
                monospaced: true,
            }]
        );
    }

    #[test]
    fn searching_filters_the_list_without_case() {
        let installed = families(&["Arial", "Georgia"]);
        let rows = font_rows(Some(&installed), "", "GEO");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].value(), "Georgia");
    }

    #[test]
    fn symbol_fonts_do_not_draw_their_own_names() {
        for name in ["Wingdings 2", "Zapf Dingbats", "Apple Color Emoji"] {
            assert!(!draws_its_own_name(name), "{name}");
        }
        assert!(draws_its_own_name("Georgia"));
    }

    #[test]
    fn an_app_font_never_replaces_a_monospace_family() {
        for family in MONOSPACE_FAMILIES {
            assert!(!PROPORTIONAL_FAMILIES.contains(family), "{family}");
        }
    }

    /// Pins the routing: a family named anywhere in the app's drawing code is either replaced by
    /// the app font or deliberately monospace, so a new one cannot slip past the pick unnoticed.
    #[test]
    fn every_named_family_is_classified() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut unknown = Vec::new();
        let mut files = vec![source];
        while let Some(path) = files.pop() {
            if path.is_dir() {
                for entry in std::fs::read_dir(&path).unwrap() {
                    files.push(entry.unwrap().path());
                }
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for line in text
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
            {
                for name in named_families(line) {
                    if !PROPORTIONAL_FAMILIES.contains(&name.as_str())
                        && !MONOSPACE_FAMILIES.contains(&name.as_str())
                    {
                        unknown.push(format!("{}: {name}", path.display()));
                    }
                }
            }
        }
        assert!(
            unknown.is_empty(),
            "unclassified font families: {unknown:#?}"
        );
    }

    /// The string literals on a line that sets a font family.
    fn named_families(line: &str) -> Vec<String> {
        const SETTERS: [&str; 5] = [
            "font_family(\"",
            "font(\"",
            "font_family: \"",
            "_FONT: &str = \"",
            "_FONT_FAMILY: &str = \"",
        ];
        SETTERS
            .iter()
            .filter_map(|setter| line.split_once(setter))
            .filter_map(|(_, rest)| rest.split_once('"'))
            .map(|(name, _)| name.to_string())
            // `ICON_FONT` and its kind name an icon file, not a family.
            .filter(|name| !name.contains('/'))
            .collect()
    }
}
