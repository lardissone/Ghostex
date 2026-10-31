//! The tray's project chip and what it opens: a search over every project on every machine, one
//! row per repository with a chip per machine. The rows are gx-core's
//! (`unified_sidebar/choices.rs`); picking moves the draft (`gx_store/unified_new_session.rs`). The
//! phone draws the same choice in `apps/mobile/app/src/chat/native/composer/ProjectPickerSheet.tsx`.
//!
//! CDXC:Worktrees 2026-10-31 DECISION:
//! User (Q847): "we need to add picking the project and machine in the bar that we added today where we added the ability to pick either the project folder or the worktree". The chip is the tray's first control; it reads the project, and the machine's name when the chat is on another machine. User (Q848): one row per repository with machine chips, stays at 100 rows; left and right arrows change the machine. The search opens on the draft's own project, so Escape leaves things as they were.

use super::{appearance::ChatAppearance, option_menu, reply_popover, state::NativeChatView};
use crate::app::gx_store::unified_new_session::PendingDraftMove;
use crate::app::native_chat::cursor::ChatCursor as _;
use ghostex_gx_core::unified_sidebar::{
    UnifiedProjectChoice, filter_project_choices, move_project_choice_machine,
};
use gpui::Focusable as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AppContext as _, Bounds, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Point, Render, ScrollHandle,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, WindowBounds,
    WindowOptions, div, px, size, svg,
};
use gpui_component::ElementExt as _;
use gpui_component::Root;
use gpui_component::input::{Input, InputEvent, InputState};
use std::time::Duration;
// `web_time`, because the web build has no `std::time::Instant` (it panics in wasm).
use web_time::Instant;

/// The card's width before the chat's zoom.
const CARD_WIDTH: f32 = 380.0;
/// The search row, a list row, the list's padding and its tallest height, before the zoom.
const SEARCH_HEIGHT: f32 = 34.0;
const ROW_HEIGHT: f32 = 30.0;
const LIST_PADDING: f32 = 4.0;
const LIST_MAX_HEIGHT: f32 = 280.0;
const CARD_RADIUS: f32 = 10.0;
/// The gap between the card and the chip it opens from.
const CHIP_GAP: f32 = 6.0;
/// A press on the chip that closed the open card (it took key status from it) is the chip's own
/// toggle: its click, a moment later, must not open it again.
const CHIP_PRESS: Duration = Duration::from_millis(400);

/*
CDXC:Worktrees 2026-10-31 DECISION:
User: the tray must not let the chat show through (AGENTS.md: anything floating over content uses the
heavily blurred tooltip background, never a faint fill). The search was drawn inside the chat window
with the composer's wash, which window glass makes nearly clear, so the transcript read through it.
GPUI blurs only behind a whole window, so the search is now a small child window of its own, opened
above the chip like the Reply popover (reply_popover.rs): frosted menu material (`menu_surface`, the
window blurred) under glass, the solid menu tone without glass. Its field types directly in that
window, so the main window no longer routes keys to it.
*/
/// The search's window while it is open, and when a key loss closed it.
#[derive(Default)]
pub(crate) struct DraftProjectPicker {
    handle: Option<gpui::WindowHandle<Root>>,
    closed_at: Option<Instant>,
}

/// The search card in its window.
struct ProjectPickerCard {
    chat: WeakEntity<NativeChatView>,
    choices: Vec<UnifiedProjectChoice>,
    input: Entity<InputState>,
    query: String,
    highlighted: usize,
    scroll: ScrollHandle,
    focus: FocusHandle,
    appearance: ChatAppearance,
    closed: bool,
    was_active: bool,
    /// True until the press on the chip that opened it has aged out.
    settling: bool,
    rekeys: u32,
    _subscriptions: Vec<Subscription>,
}

impl ProjectPickerCard {
    fn rows(&self) -> Vec<UnifiedProjectChoice> {
        filter_project_choices(&self.choices, &self.query)
    }
}

/// The card's height for `rows` rows, the list capped at its tallest.
fn card_height(rows: usize, scale: f32) -> f32 {
    let list = (rows.max(1) as f32 * ROW_HEIGHT + LIST_PADDING * 2.0).min(LIST_MAX_HEIGHT);
    (SEARCH_HEIGHT + list + 2.0) * scale
}

/// Where the card opens: its left edge on the chip's and its bottom just above it, kept inside the
/// chat.
fn card_bounds(
    chip: Bounds<Pixels>,
    within: Bounds<Pixels>,
    height: f32,
    scale: f32,
) -> Bounds<Pixels> {
    let margin = px(8.0 * scale);
    let width = px(CARD_WIDTH * scale).min(within.size.width - margin * 2.0);
    let height = px(height).min(chip.top() - within.top() - margin);
    let x = chip
        .left()
        .min(within.right() - width - margin)
        .max(within.left() + margin);
    let y = (chip.top() - px(CHIP_GAP * scale) - height).max(within.top() + margin);
    Bounds::new(Point::new(x, y), size(width, height))
}

impl NativeChatView {
    /// What the chip reads.
    pub(super) fn draft_project_chip_label(&self, name: &str, cx: &Context<Self>) -> String {
        let machine = (self.config.machine_id != ghostex_gx_core::LOCAL_MACHINE_ID)
            .then(|| {
                let app = self.config.app.as_ref()?.upgrade()?;
                let inputs = &app.read(cx).gx_store.sidebar_list.last_inputs;
                inputs
                    .host
                    .machines
                    .iter()
                    .find(|tab| tab.machine_id == self.config.machine_id)
                    .map(|tab| tab.label.clone())
            })
            .flatten();
        match machine {
            Some(machine) => format!("{name} · {machine}"),
            None => name.to_string(),
        }
    }

    pub(super) fn render_draft_project_chip(
        &self,
        name: &str,
        p: &ChatAppearance,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let s = p.scale;
        let label = self.draft_project_chip_label(name, cx);
        let bounds = self.draft_project_chip_bounds.clone();
        div()
            .on_prepaint(move |painted, _, _| bounds.set(painted))
            .id("chat-draft-project")
            .role(gpui::Role::Button)
            .aria_label(format!("Project: {label}"))
            .flex_shrink_0()
            .h(px(26.0 * s))
            .px(px(8.0 * s))
            .flex()
            .items_center()
            .gap(px(5.0 * s))
            .rounded(px(7.0 * s))
            .border_1()
            .border_color(p.composer_border)
            // Only its border: the worktree tray under it already has the box's fill.
            .text_color(p.primary)
            .when(!busy, |chip| {
                chip.chat_cursor_pointer()
                    .hover(|style| style.bg(p.border))
                    .on_click(cx.listener(|chat, _, window, cx| {
                        chat.toggle_draft_project_picker(window, cx);
                    }))
            })
            .child(
                svg()
                    .path("titlebar/folder.svg")
                    .size(px(13.0 * s))
                    .flex_shrink_0()
                    .text_color(p.muted),
            )
            .child(
                div()
                    .max_w(px(180.0 * s))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(label),
            )
            .child(
                svg()
                    .path("titlebar/chevron-down.svg")
                    .size(px(12.0 * s))
                    .flex_shrink_0()
                    .text_color(p.muted),
            )
            .into_any_element()
    }

    /// The chip's click: opens the search, or closes the one open (also when the press that started
    /// this click already closed it by taking key status, `CHIP_PRESS`).
    fn toggle_draft_project_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.draft_project_picker.take() {
            Some(DraftProjectPicker {
                handle: Some(handle),
                ..
            }) => {
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                });
                return;
            }
            Some(DraftProjectPicker {
                closed_at: Some(at),
                ..
            }) if at.elapsed() < CHIP_PRESS => return,
            _ => {}
        }
        let Some(app) = self.config.app.as_ref().and_then(|app| app.upgrade()) else {
            return;
        };
        let choices = app.read(cx).gx_store_project_choices();
        let project_id = self.config.project_id.clone();
        let highlighted = choices
            .iter()
            .position(|choice| {
                choice
                    .machines
                    .iter()
                    .any(|machine| machine.project_id == project_id)
            })
            .unwrap_or(0);
        let appearance = ChatAppearance::current(&self.snapshot);
        let scale = appearance.scale;
        let content = super::child_window::content_bounds(window);
        let chip = self.draft_project_chip_bounds.get();
        let view = self.bounds.get();
        let within = if view.size.width > px(0.0) && view.size.height > px(0.0) {
            Bounds::new(content.origin + view.origin, view.size)
        } else {
            content
        };
        let bounds = card_bounds(
            Bounds::new(content.origin + chip.origin, chip.size),
            within,
            card_height(choices.len(), scale),
            scale,
        );
        let parent = super::child_window::window_native_view(window)
            .unwrap_or(self.config.parent_native_view);
        let display_id =
            crate::app::window::popup_frame::PopupOwner::of(window, cx).display_for(bounds, cx);
        let glass = crate::app::helpers::window_glass_active();
        let corner_radius =
            crate::app::window::popup_frame::popup_card_radius(px(CARD_RADIUS * scale), glass);
        let chat = cx.weak_entity();
        self.draft_project_picker = Some(DraftProjectPicker::default());
        cx.defer(move |cx| {
            let result = cx.open_window(
                WindowOptions {
                    kind: crate::app::window::popup_frame::child_window_kind(),
                    window_decorations: crate::app::window::popup_frame::child_window_decorations(),
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    display_id,
                    titlebar: None,
                    focus: true,
                    show: true,
                    is_movable: false,
                    is_resizable: false,
                    is_minimizable: false,
                    app_id: crate::gpui_platform_window_app_id(),
                    icon: crate::gpui_platform_window_icon(),
                    window_background: if glass {
                        gpui::WindowBackgroundAppearance::Blurred
                    } else {
                        gpui::WindowBackgroundAppearance::Transparent
                    },
                    ..Default::default()
                },
                {
                    let chat = chat.clone();
                    move |window, cx| {
                        crate::app::window::popup_frame::round_popup_window(
                            window,
                            corner_radius,
                            glass,
                        );
                        crate::app::helpers::apply_frosted_menu_blur(window);
                        crate::app::window::popup_frame::strip_gpui_popup_window_frame(window);
                        crate::app::window::attach_gpui_app_modal_window_to_main_window(
                            window, parent,
                        );
                        let card = cx.new(|cx| {
                            ProjectPickerCard::new(
                                chat,
                                choices,
                                highlighted,
                                appearance,
                                window,
                                cx,
                            )
                        });
                        cx.new(|cx| {
                            Root::new(card, window, cx)
                                .bordered(false)
                                .bg(gpui::transparent_black())
                        })
                    }
                },
            );
            let _ = chat.update(cx, |chat, cx| match result {
                Ok(handle) => {
                    chat.draft_project_picker = Some(DraftProjectPicker {
                        handle: Some(handle),
                        closed_at: None,
                    });
                }
                Err(error) => {
                    chat.draft_project_picker = None;
                    chat.error = Some(error.to_string());
                    cx.notify();
                }
            });
        });
    }

    /// The search's window, which counts as one of the chat's own child windows.
    pub(super) fn draft_project_picker_window(&self) -> Option<gpui::WindowId> {
        self.draft_project_picker
            .as_ref()
            .and_then(|picker| picker.handle)
            .map(|handle| handle.window_id())
    }

    /// Takes the search down without picking (the pane went away).
    pub(super) fn close_draft_project_picker(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self
            .draft_project_picker
            .take()
            .and_then(|picker| picker.handle)
        {
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            });
        }
    }

    fn pick_draft_project(
        &mut self,
        row: &UnifiedProjectChoice,
        chip: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(machine) = row.machines.get(chip) else {
            return;
        };
        // A machine that is away can't start anything yet; its chip is dimmed.
        if !machine.connected {
            return;
        }
        let same = machine.project_id == self.config.project_id
            && machine.machine_id == self.config.machine_id;
        let agent_id = self.snapshot["sessionAgentId"].as_str().map(str::to_string);
        let (false, Some(agent_id), Some(app)) = (
            same,
            agent_id,
            self.config.app.as_ref().and_then(|app| app.upgrade()),
        ) else {
            return;
        };
        let draft = PendingDraftMove {
            from_machine_id: self.config.machine_id.clone(),
            from_project_id: self.config.project_id.clone(),
            from_session_id: self.config.session_id.clone(),
            text: self.draft.clone(),
        };
        let (repo_key, machine_id, project_id) = (
            row.repo_key.clone(),
            machine.machine_id.clone(),
            machine.project_id.clone(),
        );
        cx.defer(move |cx| {
            app.update(cx, |app, cx| {
                app.gx_store_move_draft_to_project(
                    draft,
                    &agent_id,
                    &repo_key,
                    &machine_id,
                    &project_id,
                    cx,
                );
            });
        });
    }
}

impl ProjectPickerCard {
    fn new(
        chat: WeakEntity<NativeChatView>,
        choices: Vec<UnifiedProjectChoice>,
        highlighted: usize,
        appearance: ChatAppearance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search projects"));
        let edits = cx.subscribe_in(
            &input,
            window,
            |card: &mut Self, input, event: &InputEvent, _, cx| match event {
                InputEvent::Change => {
                    card.query = input.read(cx).value().to_string();
                    card.highlighted = 0;
                    card.scroll.scroll_to_item(0);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    let rows = card.rows();
                    if let Some(row) = rows.get(card.highlighted) {
                        card.close(Some((row.clone(), row.selected)), cx);
                    }
                }
                _ => {}
            },
        );
        input.read(cx).focus_handle(cx).focus(window, cx);
        let activation = cx.observe_window_activation(window, |card: &mut Self, window, cx| {
            if window.is_window_active() {
                card.was_active = true;
            } else if card.was_active {
                crate::app::window::popup_dismissal::dismiss_on_focus_loss(
                    card,
                    window,
                    cx,
                    |card, window, cx| card.key_lost(window, cx),
                );
            }
        });
        cx.spawn_in(window, async move |card, cx| {
            cx.background_executor()
                .timer(reply_popover::OPENING_PRESS)
                .await;
            let _ = card.update_in(cx, |card, window, _| {
                card.settling = false;
                if !window.is_window_active() && !option_menu::key_loss_is_dismissal() {
                    card.retake_key(window);
                }
            });
        })
        .detach();
        let press = crate::app::window::popup_dismissal::observe_main_window_press(
            window,
            cx,
            |card: &mut Self, _, cx| card.close(None, cx),
        );
        let scroll = ScrollHandle::new();
        scroll.scroll_to_item(highlighted);
        Self {
            chat,
            choices,
            input,
            query: String::new(),
            highlighted,
            scroll,
            focus: cx.focus_handle(),
            appearance,
            closed: false,
            was_active: window.is_window_active(),
            settling: true,
            rekeys: 0,
            _subscriptions: vec![edits, activation, press],
        }
    }

    /// Key status went elsewhere: the user sent it there and the card closes, or something else
    /// took it and the card takes it back (the Reply popover's rule, `key_loss_closes`).
    fn key_lost(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed || window.is_window_active() {
            return;
        }
        if reply_popover::key_loss_closes(
            self.settling,
            option_menu::key_loss_is_dismissal(),
            self.rekeys,
        ) {
            self.close(None, cx);
        } else {
            self.retake_key(window);
        }
    }

    fn retake_key(&mut self, window: &mut Window) {
        self.rekeys += 1;
        window.activate_window();
    }

    /// Closes the card, gives the chat its keyboard back and moves the draft for a pick.
    fn close(&mut self, pick: Option<(UnifiedProjectChoice, usize)>, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.closed = true;
        let chat = self.chat.clone();
        cx.defer(move |cx| {
            let Ok((handle, main)) = chat.update(cx, |chat, _| {
                let handle = chat
                    .draft_project_picker
                    .take()
                    .and_then(|picker| picker.handle);
                chat.draft_project_picker = Some(DraftProjectPicker {
                    handle: None,
                    closed_at: Some(Instant::now()),
                });
                (handle, chat.main_window)
            }) else {
                return;
            };
            if let Some(handle) = handle {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
            let Some(main) = main else {
                return;
            };
            let _ = main.update(cx, |_, window, cx| {
                window.activate_window();
                let _ = chat.update(cx, |chat, cx| {
                    chat.focus_composer_from_hotkey(window, cx);
                    if let Some((row, chip)) = pick {
                        chat.pick_draft_project(&row, chip, cx);
                    }
                    cx.notify();
                });
            });
        });
    }

    /// Up and down walk the rows; left and right change the highlighted row's machine; Escape
    /// closes.
    fn key(&mut self, event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            cx.stop_propagation();
            window.prevent_default();
            self.close(None, cx);
            return;
        }
        let rows = self.rows();
        let Some(row) = rows.get(self.highlighted) else {
            return;
        };
        match key {
            "up" => self.highlighted = self.highlighted.saturating_sub(1),
            "down" => self.highlighted = (self.highlighted + 1).min(rows.len() - 1),
            "left" | "right" if row.machines.len() > 1 => {
                let selected = move_project_choice_machine(row, key == "right");
                let repo_key = row.repo_key.clone();
                if let Some(choice) = self
                    .choices
                    .iter_mut()
                    .find(|choice| choice.repo_key == repo_key)
                {
                    choice.selected = selected;
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        window.prevent_default();
        self.scroll.scroll_to_item(self.highlighted);
        cx.notify();
    }
}

impl Render for ProjectPickerCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.appearance.clone();
        let s = p.scale;
        let (wash, edge) = reply_popover::menu_ink_washes(p.light);
        let rows = self.rows();
        let highlighted = self.highlighted;
        let rows_view: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let on = index == highlighted;
                let pick_row = row.clone();
                let chips = (row.machines.len() > 1
                    || row
                        .machines
                        .first()
                        .map(|machine| machine.machine_id.as_str())
                        != Some(ghostex_gx_core::LOCAL_MACHINE_ID))
                .then(|| {
                    div()
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .gap(px(3.0 * s))
                        .children(row.machines.iter().enumerate().map(|(chip, machine)| {
                            let selected = chip == row.selected;
                            let chip_row = row.clone();
                            div()
                                .id(format!("chat-draft-project-{index}-chip-{chip}"))
                                .role(gpui::Role::Button)
                                .aria_label(machine.label.clone())
                                .h(px(20.0 * s))
                                .px(px(6.0 * s))
                                .flex()
                                .items_center()
                                .rounded(px(5.0 * s))
                                .text_size(px(11.5 * s))
                                .border_1()
                                .border_color(if selected { p.muted } else { edge })
                                .text_color(if selected { p.primary } else { p.muted })
                                .when(!machine.connected, |chip| chip.opacity(0.45))
                                .when(machine.connected, |chip| chip.chat_cursor_pointer())
                                .child(machine.label.clone())
                                .on_click(cx.listener(move |card, _, _, cx| {
                                    cx.stop_propagation();
                                    card.close(Some((chip_row.clone(), chip)), cx);
                                }))
                        }))
                });
                div()
                    .id(("chat-draft-project-row", index))
                    .role(gpui::Role::Button)
                    .aria_label(row.label.clone())
                    .min_h(px(ROW_HEIGHT * s))
                    .px(px(8.0 * s))
                    .flex()
                    .items_center()
                    .gap(px(8.0 * s))
                    .rounded(px(6.0 * s))
                    .text_size(px(13.0 * s))
                    .text_color(p.foreground)
                    .chat_cursor_pointer()
                    .when(on, |row| row.bg(wash))
                    .hover(move |style| style.bg(wash))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(row.label.clone()),
                    )
                    .children(chips)
                    .on_click(cx.listener(move |card, _, _, cx| {
                        cx.stop_propagation();
                        let chip = pick_row.selected;
                        card.close(Some((pick_row.clone(), chip)), cx);
                    }))
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        div()
            .id("chat-draft-project-picker")
            .role(gpui::Role::Dialog)
            .aria_label("Pick a project")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(CARD_RADIUS * s))
            .border_1()
            .border_color(edge)
            .bg(p.menu_surface())
            .font_family(p.font.clone())
            .text_color(p.foreground)
            .capture_key_down(cx.listener(Self::key))
            .child(
                div()
                    .h(px(SEARCH_HEIGHT * s))
                    .flex_shrink_0()
                    .px(px(10.0 * s))
                    .flex()
                    .items_center()
                    .gap(px(6.0 * s))
                    .border_b_1()
                    .border_color(edge)
                    .child(
                        svg()
                            .path("titlebar/search.svg")
                            .size(px(13.0 * s))
                            .flex_shrink_0()
                            .text_color(p.muted),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.input)
                                .appearance(false)
                                .bordered(false)
                                .focus_bordered(false)
                                .w_full()
                                .p_0()
                                .text_size(px(13.0 * s))
                                .text_color(p.foreground)
                                .placeholder_color(p.muted.opacity(0.6)),
                        ),
                    ),
            )
            .child(
                div()
                    .id("chat-draft-project-rows")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .p(px(LIST_PADDING * s))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(rows_view)
                    .when(empty, |list| {
                        list.child(
                            div()
                                .px(px(8.0 * s))
                                .py(px(8.0 * s))
                                .text_size(px(13.0 * s))
                                .text_color(p.muted)
                                .child("No projects match"),
                        )
                    }),
            )
    }
}
