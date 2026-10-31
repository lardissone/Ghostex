//! The Theme page's Font group: the app font, picked from the fonts installed on this computer.
use super::super::super::fields::{DropdownState, font_picker_field, reset_key, settings_section};
use super::controls::ICON_FONT;
use super::{ThemeCx, ThemeTab};
use crate::ui_fonts::APP_FONT_SETTING;
use gpui::{AnyElement, Context, IntoElement, Window};
use serde_json::json;

fn font_dropdown(page: &mut ThemeTab) -> &mut DropdownState {
    &mut page.font_dropdown
}

impl ThemeTab {
    pub(super) fn font_section(
        &mut self,
        t: &ThemeCx,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !t.visible(APP_FONT_SETTING) {
            return None;
        }
        let p = t.p;
        let spec = t
            .spec(
                APP_FONT_SETTING,
                "App font",
                "Used across the whole app: sidebar, chat, menus, dialogs and Settings. Terminals and code keep their monospace font.",
            )
            .icon(ICON_FONT);
        let value = t.values.string(APP_FONT_SETTING);
        let row = font_picker_field(
            self,
            &p,
            APP_FONT_SETTING,
            spec,
            Some(reset_key::<Self>(APP_FONT_SETTING)),
            &value,
            font_dropdown,
            |page: &mut Self, next, _window, cx| page.save(APP_FONT_SETTING, json!(next), cx),
            window,
            cx,
        );
        settings_section(&p, "Font", None, None, vec![row]).map(IntoElement::into_any_element)
    }
}
