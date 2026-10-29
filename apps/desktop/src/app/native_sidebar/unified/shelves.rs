//! The folded shelves under the list: Working, Snoozed and Parked.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    div, px,
};
use gpui_component::{h_flex, v_flex};
use serde_json::Value;

use ghostex_gx_core::unified_sidebar::UnifiedShelf;

use super::cards::{now_ms, repo_tile};
use super::model::{UnifiedCard, UnifiedShelfView};
use crate::GhostexGpuiApp;
use crate::app::helpers::chrome_color;
use crate::app::native_sidebar::appearance::SidebarAppearance;

fn shelf_label(shelf: UnifiedShelf) -> &'static str {
    match shelf {
        UnifiedShelf::Working => "Working",
        UnifiedShelf::Snoozed => "Snoozed",
        UnifiedShelf::Parked => "Parked",
        UnifiedShelf::Pinned => "Pinned",
        UnifiedShelf::Active => "Sessions",
    }
}

/// `in 12m`, `in 3h`, `in 2d`: when a snoozed session wakes.
fn wake_label(wake_at_ms: i64, now: i64) -> String {
    let minutes = ((wake_at_ms - now) / 60_000).max(1);
    match minutes {
        0..60 => format!("in {minutes}m"),
        60..1440 => format!("in {}h", minutes / 60),
        _ => format!("in {}d", minutes / 1440),
    }
}

impl GhostexGpuiApp {
    /// A shelf: its header, and its rows while open. Working keeps the full cards; Snoozed and
    /// Parked rows are one line.
    pub(in crate::app::native_sidebar) fn render_native_unified_shelf(
        &self,
        shelf: &UnifiedShelfView,
        hud: &Value,
        appearance: &SidebarAppearance,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let scale = appearance.scale;
        let kind = shelf.shelf;
        let open = self.native_sidebar.unified_open_shelves.contains(&kind);
        v_flex()
            .w_full()
            .child(
                h_flex()
                    .id(format!("native-unified-shelf-{}", shelf_label(kind)))
                    .role(gpui::Role::Button)
                    .aria_label(format!("{} ({})", shelf_label(kind), shelf.cards.len()))
                    .aria_expanded(open)
                    .mx(px(3.0 * scale))
                    .h(px(30.0 * scale))
                    .px(px(6.0 * scale))
                    .gap(px(6.0 * scale))
                    .rounded(px(5.0 * scale))
                    .text_size(px(13.0 * scale))
                    .font_weight(gpui::FontWeight::NORMAL)
                    .text_color(appearance.muted)
                    .hover(|row| row.bg(appearance.hover))
                    .child(
                        gpui::svg()
                            .path(if open {
                                "titlebar/chevron-down.svg"
                            } else {
                                "titlebar/chevron-right.svg"
                            })
                            .size(px(13.0 * scale))
                            .text_color(appearance.muted),
                    )
                    .child(div().flex_1().child(shelf_label(kind)))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::LIGHT)
                            .child(shelf.cards.len().to_string()),
                    )
                    .on_click(cx.listener(move |app, _, _, cx| {
                        cx.stop_propagation();
                        let shelves = &mut app.native_sidebar.unified_open_shelves;
                        if !shelves.remove(&kind) {
                            shelves.insert(kind);
                        }
                        cx.notify();
                    })),
            )
            .when(open, |column| {
                column.children(shelf.cards.iter().map(|card| {
                    if kind == UnifiedShelf::Working {
                        self.render_native_unified_card(card, hud, appearance, cx)
                    } else {
                        self.render_native_unified_slim_row(card, kind, appearance, cx)
                    }
                }))
            })
            .into_any_element()
    }

    /// One line: the greyed project tile, the title, and when it wakes or was last active.
    fn render_native_unified_slim_row(
        &self,
        card: &UnifiedCard,
        shelf: UnifiedShelf,
        appearance: &SidebarAppearance,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let scale = appearance.scale;
        let now = now_ms();
        let wake = (shelf == UnifiedShelf::Snoozed)
            .then_some(card.row.timing.snoozed_until_ms)
            .flatten();
        let (focused, _) = self.gx_store_sidebar_row_focus(
            &card.session.session_id,
            false,
            card.session.is_focused,
            card.session.is_visible,
        );
        let trailing = match wake {
            Some(wake_at) => h_flex()
                .flex_shrink_0()
                .gap(px(4.0 * scale))
                .text_size(px(12.5 * scale))
                .text_color(chrome_color(0x95d7f6, 0x2f8fd0))
                .child(
                    gpui::svg()
                        .path("titlebar/alarm.svg")
                        .size(px(13.0 * scale))
                        .text_color(chrome_color(0x95d7f6, 0x2f8fd0)),
                )
                .child(wake_label(wake_at, now))
                .into_any_element(),
            None => div()
                .flex_shrink_0()
                .text_size(px(13.55 * scale))
                .text_color(chrome_color(0xa6a6a6, 0x424242))
                .child(
                    card.row
                        .last_interaction_label(now.max(0) as u64)
                        .unwrap_or_default(),
                )
                .into_any_element(),
        };
        let row = h_flex()
            .id(format!("native-unified-slim-{}", card.session.session_id))
            .role(gpui::Role::TreeItem)
            .aria_label(card.session.title().to_owned())
            .aria_selected(focused)
            .h(px(34.0 * scale))
            .px(px(8.0 * scale))
            .gap(px(8.0 * scale))
            .rounded(px(5.0 * scale))
            .when(focused, |row| {
                row.bg(appearance.session_selected)
                    .border_1()
                    .border_color(appearance.session_outline)
            })
            .when(!focused, |row| {
                row.hover(|row| row.bg(appearance.session_hover))
            })
            .child(repo_tile(&card.repo_label, appearance, true))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .opacity(0.8)
                    .child(card.session.title().to_owned()),
            )
            .child(trailing);
        div()
            .w_full()
            .pb(px(1.0 * scale))
            .px(px(3.0 * scale))
            .child(self.unified_row_interactions(row, card, cx))
            .into_any_element()
    }
}
