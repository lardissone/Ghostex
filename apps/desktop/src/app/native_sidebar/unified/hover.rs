//! What a hovered Unified card shows where its status was: Snooze and Park on a card, Unsnooze or
//! Unpark on a shelf's one-line row.
//!
//! CDXC:Sidebar 2026-10-30 DECISION:
//! User: snooze and park buttons on Unified sidebar cards, matching the inbox sidebar the Unified one is modelled on. Hovering a card swaps its status for a clock (Snooze) and a check with the word Park, small and muted, brightening under the pointer; Snooze opens the session's own snooze choices (1 Hour, 3 Hours, Tomorrow, Next Week, the session menu's Snooze submenu) and the card moves to the Snoozed shelf until it wakes; Park parks it into the Parked shelf. Park shows only while Enable session parking is on, since a parked card has no shelf otherwise. A snoozed row offers Unsnooze and a parked row Unpark in the same place.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    px,
};
use gpui_component::{ElementExt as _, h_flex};
use serde_json::{Value, json};

use ghostex_gx_core::unified_sidebar::UnifiedShelf;

use super::model::UnifiedCard;
use crate::GhostexGpuiApp;
use crate::app::native_sidebar::actions::NativeSidebarAction;
use crate::app::native_sidebar::appearance::SidebarAppearance;
use crate::app::native_sidebar::menus::{MenuOwner, press_position, record_menu_owner};

/// A session command as the session menu's rows send it.
fn session_command(message: Value) -> Value {
    json!({"type": "command", "message": message})
}

impl GhostexGpuiApp {
    /// One muted button: an icon, and a word when it has one.
    fn unified_hover_button(
        id: String,
        icon: &'static str,
        word: Option<&'static str>,
        label: &'static str,
        appearance: &SidebarAppearance,
    ) -> gpui::Stateful<gpui::Div> {
        let scale = appearance.scale;
        let hover_color = appearance.foreground;
        h_flex()
            .id(id)
            .role(gpui::Role::Button)
            .aria_label(label)
            .flex_shrink_0()
            .h(px(20.0 * scale))
            .px(px(6.0 * scale))
            .gap(px(4.0 * scale))
            .rounded(px(5.0 * scale))
            .cursor_pointer()
            .text_size(px(12.0 * scale))
            .text_color(appearance.muted)
            .hover(move |button| button.text_color(hover_color).bg(appearance.hover))
            .child(
                gpui::svg()
                    .path(icon)
                    .size(px(13.0 * scale))
                    .flex_shrink_0()
                    .text_color(appearance.muted),
            )
            .children(word)
    }

    /// Snooze and Park, for a card in the list, Drafts or the Working shelf.
    pub(super) fn render_unified_card_actions(
        &self,
        card: &UnifiedCard,
        parking: bool,
        appearance: &SidebarAppearance,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let scale = appearance.scale;
        let id = card.session.session_id.clone();
        let owner = MenuOwner::default();
        let menu_owner = format!("session:{id}");
        // The session's own Snooze submenu, asked for when it opens (`sessionMenu`).
        let snooze_menu = json!([{
            "label": "Loading…",
            "disabled": true,
            "menuOwner": menu_owner,
            "onOpen": {"type": "sessionMenu", "sessionId": id, "action": "snooze", "ownerId": menu_owner},
        }]);
        let trigger = format!("unified-snooze:{id}");
        let snooze = Self::unified_hover_button(
            format!("native-unified-snooze-{id}"),
            "titlebar/clock.svg",
            None,
            "Snooze session",
            appearance,
        )
        .on_prepaint(record_menu_owner(&owner))
        .on_click(cx.listener(move |_, event: &gpui::ClickEvent, window, cx| {
            cx.stop_propagation();
            Self::show_native_sidebar_menu_hung(
                &snooze_menu,
                owner.get(),
                None,
                Some((trigger.clone(), press_position(event))),
                event.position(),
                scale,
                window,
                cx,
            );
        }));
        let park_id = id.clone();
        h_flex()
            .flex_shrink_0()
            .gap(px(2.0 * scale))
            .child(snooze)
            .when(parking, |row| {
                row.child(
                    Self::unified_hover_button(
                        format!("native-unified-park-{id}"),
                        "titlebar/check.svg",
                        Some("Park"),
                        "Park session",
                        appearance,
                    )
                    .on_click(cx.listener(move |app, _, window, cx| {
                        cx.stop_propagation();
                        app.handle_native_sidebar_action(
                            &NativeSidebarAction {
                                command: session_command(json!({
                                    "type": "setSessionParked",
                                    "sessionId": park_id,
                                    "parked": true,
                                })),
                            },
                            window,
                            cx,
                        );
                    })),
                )
            })
            .into_any_element()
    }

    /// Unsnooze on a snoozed row, Unpark on a parked one.
    pub(super) fn render_unified_shelf_row_action(
        &self,
        card: &UnifiedCard,
        shelf: UnifiedShelf,
        appearance: &SidebarAppearance,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        let id = card.session.session_id.clone();
        let (icon, word, label, message) = match shelf {
            UnifiedShelf::Snoozed => (
                "titlebar/alarm.svg",
                "Unsnooze",
                "Unsnooze session",
                json!({"type": "unsnoozeSession", "sessionId": id}),
            ),
            UnifiedShelf::Parked => (
                "titlebar/arrow-back-up.svg",
                "Unpark",
                "Unpark session",
                json!({"type": "setSessionParked", "sessionId": id, "parked": false}),
            ),
            _ => return None,
        };
        Some(
            Self::unified_hover_button(
                format!("native-unified-{word}-{id}"),
                icon,
                Some(word),
                label,
                appearance,
            )
            .on_click(cx.listener(move |app, _, window, cx| {
                cx.stop_propagation();
                app.handle_native_sidebar_action(
                    &NativeSidebarAction {
                        command: session_command(message.clone()),
                    },
                    window,
                    cx,
                );
            }))
            .into_any_element(),
        )
    }
}
