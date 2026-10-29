//! The Unified sidebar's header: the filter, Add project and New session.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    div, px,
};
use gpui_component::h_flex;
use serde_json::{Value, json};

use super::model::UnifiedSidebarSnapshot;
use crate::GhostexGpuiApp;
use crate::app::native_sidebar::appearance::SidebarAppearance;

/// The filter menu: projects (by repository), machines and Spaces, each a pick that narrows the
/// list, ticked while it does.
///
/// CDXC:Sidebar 2026-10-30 DECISION:
/// User (Q788): the header's filter narrows by projects, machines and Spaces in one menu, which stands in for the machine tabs and Space tiles the Unified sidebar does not draw. User (Q787): there is no Group by; the list stays flat.
pub(super) fn filter_menu_items(unified: &UnifiedSidebarSnapshot) -> Value {
    let filter = &unified.filter;
    let mut items = vec![json!({
        "label": "All sessions",
        "checked": filter.is_empty(),
        "command": {"type": "unifiedFilter", "kind": "clear"},
    })];
    let mut section = |title: &str,
                       kind: &str,
                       options: &[ghostex_gx_core::unified_sidebar::UnifiedFilterOption],
                       picked: &std::collections::BTreeSet<String>| {
        if options.len() < 2 && picked.is_empty() {
            return;
        }
        items.push(json!({"separator": true}));
        items.push(json!({"label": title, "heading": true}));
        for option in options {
            let mut item = json!({
                "label": option.label,
                "checked": picked.contains(&option.key),
                "command": {"type": "unifiedFilter", "kind": kind, "key": option.key},
            });
            if let Some(detail) = &option.detail {
                item["suffix"] = Value::String(detail.clone());
            }
            items.push(item);
        }
    };
    // The two short lists first, so they are not lost below a long list of projects.
    section(
        "Machines",
        "machine",
        &unified.options.machines,
        &filter.machines,
    );
    section("Spaces", "space", &unified.options.spaces, &filter.spaces);
    section("Projects", "repo", &unified.options.repos, &filter.repos);
    Value::Array(items)
}

/// What the filter button says: everything, or the first pick and how many more.
fn filter_label(unified: &UnifiedSidebarSnapshot) -> String {
    let filter = &unified.filter;
    let picked: Vec<&str> = unified
        .options
        .repos
        .iter()
        .filter(|option| filter.repos.contains(&option.key))
        .chain(
            unified
                .options
                .machines
                .iter()
                .filter(|option| filter.machines.contains(&option.key)),
        )
        .chain(
            unified
                .options
                .spaces
                .iter()
                .filter(|option| filter.spaces.contains(&option.key)),
        )
        .map(|option| option.label.as_str())
        .collect();
    match picked.as_slice() {
        [] => "All sessions".to_string(),
        [one] => (*one).to_string(),
        [first, rest @ ..] => format!("{first} +{}", rest.len()),
    }
}

impl GhostexGpuiApp {
    pub(in crate::app::native_sidebar) fn render_native_unified_header(
        &self,
        unified: &UnifiedSidebarSnapshot,
        appearance: &SidebarAppearance,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let scale = appearance.scale;
        let icon_button = |id: &'static str, label: &'static str, icon: &'static str| {
            div()
                .id(id)
                .role(gpui::Role::Button)
                .aria_label(label)
                .size(px(26.0 * scale))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.0 * scale))
                .hover(|button| button.bg(appearance.hover))
                .child(
                    gpui::svg()
                        .path(icon)
                        .size(px(15.0 * scale))
                        .text_color(appearance.chrome_icon),
                )
        };
        let filtered = !unified.filter.is_empty();
        let menu = filter_menu_items(unified);
        h_flex()
            .w_full()
            .flex_shrink_0()
            .h(px(36.0 * scale))
            .px(px(8.0 * scale))
            .gap(px(2.0 * scale))
            .child(
                h_flex()
                    .id("native-unified-filter")
                    .role(gpui::Role::Button)
                    .aria_label("Filter sessions")
                    .min_w_0()
                    .flex_1()
                    .h(px(28.0 * scale))
                    .px(px(8.0 * scale))
                    .gap(px(7.0 * scale))
                    .rounded(px(6.0 * scale))
                    .text_size(px(13.5 * scale))
                    .text_color(if filtered {
                        appearance.foreground
                    } else {
                        appearance.muted
                    })
                    .when(filtered, |button| button.bg(appearance.selected))
                    .hover(|button| button.bg(appearance.hover))
                    .child(
                        gpui::svg()
                            .path("titlebar/filter.svg")
                            .size(px(14.0 * scale))
                            .flex_shrink_0()
                            .text_color(appearance.chrome_icon),
                    )
                    .child(div().min_w_0().truncate().child(filter_label(unified)))
                    .child(
                        gpui::svg()
                            .path("titlebar/chevron-down.svg")
                            .size(px(12.0 * scale))
                            .flex_shrink_0()
                            .text_color(appearance.muted),
                    )
                    .on_click(cx.listener(move |_, event: &gpui::ClickEvent, window, cx| {
                        cx.stop_propagation();
                        Self::show_native_sidebar_context_menu(
                            &menu,
                            None,
                            event.position(),
                            scale,
                            window,
                            cx,
                        );
                    })),
            )
            .child(
                icon_button(
                    "native-unified-add-project",
                    "Add project",
                    "titlebar/folder-plus.svg",
                )
                .on_click(cx.listener(|app, _, _, cx| {
                    cx.stop_propagation();
                    app.dispatch_native_sidebar_ui(
                        json!({"type": "sidebarAction", "action": "addProject"}),
                        cx,
                    );
                })),
            )
            .child(
                icon_button(
                    "native-unified-new-session",
                    "New session",
                    "titlebar/edit.svg",
                )
                .on_click(cx.listener(|app, _, _, cx| {
                    cx.stop_propagation();
                    app.dispatch_native_sidebar_command(json!({"type": "createSession"}), cx);
                })),
            )
            .into_any_element()
    }
}
