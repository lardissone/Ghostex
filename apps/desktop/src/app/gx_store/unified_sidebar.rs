//! The Unified sidebar's store half: one neutral list per machine (every Space, no filter,
//! collapse or hide), drawn with the classic rows' own snapshot builder so every card carries the
//! same menu, hover buttons and icon, then put in one list by gx-core (`unified_sidebar/`).
//!
//! Nothing here runs while the Project sidebar is selected: the lanes are dropped and the renderer
//! draws the classic list it always did. The web build compiles this file too.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use ghostex_gx_core::indicators::neutral_indicator_inputs;
use ghostex_gx_core::unified_sidebar::{
    UnifiedFilter, UnifiedMachineInput, UnifiedRow, UnifiedSettings, UnifiedShelf,
    build_unified_sidebar, repo_of_project, unified_machine_spaces,
};
use ghostex_gx_core::{
    ChangeSummary, Core, HoverAction, LOCAL_MACHINE_ID, MachineId, MenuHost, MenuItem, SessionKey,
    SidebarInputs, SidebarMenus, SidebarViewModel,
};
use serde_json::Value;

use super::sidebar_snapshot::{SnapshotCache, SnapshotInput, snapshot_from_view};
use crate::GhostexGpuiApp;
use crate::app::native_sidebar::model::{NativeSidebarGroup, NativeSidebarSession};
use crate::app::native_sidebar::unified::model::{
    UnifiedCard, UnifiedShelfView, UnifiedSidebarSnapshot,
};

/// One machine's neutral list and the snapshot cache its rows are drawn through.
#[derive(Default)]
struct MachineLane {
    model: SidebarViewModel,
    cache: SnapshotCache,
    inputs: SidebarInputs,
}

#[derive(Default)]
pub(crate) struct UnifiedSidebarHost {
    lanes: BTreeMap<MachineId, MachineLane>,
    /// What the header's filter keeps; kept while the app runs.
    pub(crate) filter: UnifiedFilter,
    /// A project picked in Quick Access whose repository the filter is set to once the list
    /// holds it (a reopened project arrives a moment later).
    pending_project: Option<String>,
}

impl UnifiedSidebarHost {
    /// The store key of a row the Unified sidebar draws, which the Project sidebar's list may not
    /// hold (a session in another Space). The web build resolves a clicked row through it; the
    /// desktop parses the row id instead.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn session_key_for_row(&self, sidebar_session_id: &str) -> Option<SessionKey> {
        self.lanes
            .values()
            .flat_map(|lane| lane.model.view().groups.iter())
            .flat_map(|group| group.core.sessions.iter())
            .find(|session| session.row.sidebar_session_id == sidebar_session_id)
            .and_then(|session| session.row.key.clone())
    }

    /// The repository of a project on whichever machine lists it.
    fn repo_key_of_project(&self, project_id: &str) -> Option<String> {
        self.lanes
            .values()
            .find_map(|lane| repo_of_project(lane.model.view(), project_id))
            .map(|repo| repo.key)
    }

    /// A project of this computer in the repository `repo_key`, for a new session to start in.
    fn local_project_in_repo(&self, repo_key: &str) -> Option<String> {
        let lane = self.lanes.get(&MachineId::Local)?;
        lane.model
            .view()
            .groups
            .iter()
            .filter_map(|group| group.core.project_context.as_ref())
            .map(|context| context.project_id.as_str())
            .find(|project_id| {
                repo_of_project(lane.model.view(), project_id)
                    .is_some_and(|repo| repo.key == repo_key)
            })
            .map(str::to_string)
    }

    /// A card's right-click menu, or one of its hover buttons' submenus, from the machine list
    /// that holds the session. `None` while the Unified sidebar is off (it has no lists then) or
    /// when no list holds the row.
    ///
    /// CDXC:Sidebar 2026-10-30 WHY:
    /// User: "right clicking only works on the currently active session, doesn't work on other one". A card's menu opens as a stub that asks the store for its items (`sessionMenu`), and the store answered only from the Project sidebar's own list: the selected Space's drawn rows, without other Spaces, other machines or folded projects. Any card it did not hold got an empty answer, which closes the menu. The Unified lists hold every session, so they answer first while the Unified sidebar is on.
    pub(crate) fn row_menu(
        &self,
        core: &Core,
        menu_host: &MenuHost,
        sidebar_session_id: &str,
        action: Option<HoverAction>,
        now_ms: u64,
    ) -> Option<Vec<Value>> {
        self.lanes.values().find_map(|lane| {
            let menus = SidebarMenus::new(core, lane.model.view(), &lane.inputs, menu_host, now_ms);
            let items = match action {
                // A card's Snooze button opens the submenu whatever the hover strip shows.
                Some(action) => menus.row_action_submenu(sidebar_session_id, action),
                None => menus.row_menu(sidebar_session_id),
            }?;
            (!items.is_empty()).then(|| items.iter().map(MenuItem::to_json).collect())
        })
    }

    /// Brings every machine's list up to date with `changes` and builds the Unified list.
    #[allow(clippy::too_many_arguments)]
    fn update(
        &mut self,
        core: &Core,
        list_inputs: &SidebarInputs,
        changes: &ChangeSummary,
        menu_host: &MenuHost,
        hud: &Arc<Value>,
        now_ms: u64,
    ) -> UnifiedSidebarSnapshot {
        let store = core.presentation();
        // This computer always, then every remote machine the host feeds into the store, under the
        // names their tabs carry.
        let mut machines: Vec<(MachineId, String)> = vec![(MachineId::Local, "Local".to_string())];
        for tab in &list_inputs.host.machines {
            if tab.machine_id == LOCAL_MACHINE_ID {
                machines[0].1.clone_from(&tab.label);
                continue;
            }
            let machine = MachineId::Remote(tab.machine_id.clone());
            if tab.fed && !tab.this_computer && store.loaded(&machine).is_some() {
                machines.push((machine, tab.label.clone()));
            }
        }
        self.lanes
            .retain(|machine, _| machines.iter().any(|(id, _)| id == machine));

        let mut groups: HashMap<(MachineId, String), Arc<NativeSidebarGroup>> = HashMap::new();
        let mut sessions: HashMap<(MachineId, String), Arc<NativeSidebarSession>> = HashMap::new();
        for (machine, _) in &machines {
            let lane = self.lanes.entry(machine.clone()).or_default();
            lane.inputs = neutral_indicator_inputs(list_inputs, machine);
            lane.model.update(core, &lane.inputs, changes, now_ms);
            let menus = SidebarMenus::new(core, lane.model.view(), &lane.inputs, menu_host, now_ms);
            let snapshot = snapshot_from_view(
                lane.model.view(),
                &SnapshotInput {
                    menus: &menus,
                    hud,
                    rename_request: None,
                    reveal_request: None,
                    search_shortcut: None,
                    commands_shortcut: None,
                    settings: &lane.inputs.settings,
                    hidden_items: &lane.inputs.ui.hidden_items,
                    host: menu_host,
                    collapsed_groups: &lane.inputs.ui.collapse.collapsed_groups,
                    show_hidden: lane.inputs.ui.show_hidden,
                    selected_tag_filters: &lane.inputs.ui.selected_tag_filters,
                    now_ms,
                },
                &mut lane.cache,
            );
            for mut group in snapshot.groups {
                for session in &group.sessions {
                    sessions.insert(
                        (machine.clone(), session.session_id.clone()),
                        session.clone(),
                    );
                }
                // A card never draws its group's other rows.
                group.sessions = Vec::new();
                groups.insert((machine.clone(), group.group_id.clone()), Arc::new(group));
            }
        }

        if let Some(project_id) = self.pending_project.clone() {
            let repo = machines.iter().find_map(|(machine, _)| {
                repo_of_project(self.lanes.get(machine)?.model.view(), &project_id)
            });
            if let Some(repo) = repo {
                self.filter = UnifiedFilter {
                    repos: [repo.key].into(),
                    ..UnifiedFilter::default()
                };
                self.pending_project = None;
            }
        }
        let inputs: Vec<UnifiedMachineInput<'_>> = machines
            .iter()
            .filter_map(|(machine, label)| {
                Some(UnifiedMachineInput {
                    machine: machine.clone(),
                    label: label.clone(),
                    view: self.lanes.get(machine)?.model.view(),
                    spaces: unified_machine_spaces(store, list_inputs, machine),
                })
            })
            .collect();
        let settings = UnifiedSettings {
            working_section: list_inputs.settings.unified_working_section,
            enable_session_parking: list_inputs.settings.enable_session_parking,
        };
        let has_projects = inputs.iter().any(|input| !input.view.groups.is_empty());
        let view = build_unified_sidebar(&inputs, settings, &self.filter, now_ms);
        // One icon per repository: the first of its projects, on any machine, that has one.
        let mut icons: HashMap<String, Arc<gpui::Image>> = HashMap::new();
        for row in view
            .drafts
            .iter()
            .chain(&view.pinned)
            .chain(&view.active)
            .chain(&view.working)
            .chain(&view.snoozed)
            .chain(&view.parked)
        {
            if icons.contains_key(&row.repo.key) {
                continue;
            }
            let icon = groups
                .get(&(row.machine.clone(), row.group.core.group_id.clone()))
                .and_then(|group| project_icon(group.project_context.as_ref()));
            if let Some(icon) = icon {
                icons.insert(row.repo.key.clone(), icon);
            }
        }
        let card = |row: UnifiedRow| -> Option<UnifiedCard> {
            Some(UnifiedCard {
                session: sessions
                    .get(&(
                        row.machine.clone(),
                        row.session.row.sidebar_session_id.clone(),
                    ))?
                    .clone(),
                row: row.session.row.clone(),
                repo_icon: icons.get(&row.repo.key).cloned(),
                repo_label: row.repo.label,
                machine_label: row.machine_label,
                status: row.status,
            })
        };
        let drafts = view.drafts.into_iter().filter_map(card).collect();
        let list = view
            .pinned
            .into_iter()
            .chain(view.active)
            .filter_map(card)
            .collect();
        let shelves = [
            (UnifiedShelf::Working, view.working),
            (UnifiedShelf::Snoozed, view.snoozed),
            (UnifiedShelf::Parked, view.parked),
        ]
        .into_iter()
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(shelf, rows)| UnifiedShelfView {
            shelf,
            cards: rows.into_iter().filter_map(card).collect(),
        })
        .collect();
        UnifiedSidebarSnapshot {
            drafts,
            list,
            shelves,
            options: view.options,
            filter: self.filter.clone(),
            total: view.total,
            has_projects,
            parking: list_inputs.settings.enable_session_parking,
        }
    }
}

impl GhostexGpuiApp {
    /// The list moved: rebuild the Unified sidebar when it is the one selected, or let it go when
    /// it is not. `changes` is what the store changed since the previous call.
    pub(crate) fn gx_store_unified_sidebar_changed(
        &mut self,
        changes: &ChangeSummary,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self
            .gx_store
            .sidebar_list
            .last_inputs
            .settings
            .unified_sidebar
        {
            if self.native_sidebar.unified.is_some() {
                self.gx_store.unified_sidebar.lanes.clear();
                self.native_sidebar.unified = None;
                cx.notify();
            }
            return;
        }
        // Without the HUD nothing can be drawn, and the lanes would miss this change: they start
        // over once it is back.
        let Some(hud) = self.gx_store_sidebar_hud() else {
            self.gx_store.unified_sidebar.lanes.clear();
            return;
        };
        let menu_host = self.gx_store_menu_host();
        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or_default();
        let store = &mut self.gx_store;
        let snapshot = store.unified_sidebar.update(
            &store.core,
            &store.sidebar_list.last_inputs,
            changes,
            &menu_host,
            &hud,
            now_ms,
        );
        self.native_sidebar.unified = Some(Arc::new(snapshot));
        cx.notify();
    }

    /// A project picked in Quick Access › Projects: while the Unified sidebar is on, its filter
    /// shows that project's repository alone, on every machine. The Project sidebar ignores it.
    ///
    /// CDXC:Sidebar 2026-10-30 DECISION:
    /// User: "when i click on a project here and unified sidebar is active then we should filter by that project in the unified sidebar". Opening or reopening a project from Quick Access › Projects (click or Enter) sets the Unified filter to that project's repository, as picking it in the filter menu would, and replaces any machine or Space pick so every machine's copy shows; the pick still opens the project as before. A reopened project is matched once it is back in the list.
    pub(crate) fn gx_store_unified_filter_to_project(
        &mut self,
        project_id: &str,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self
            .gx_store
            .sidebar_list
            .last_inputs
            .settings
            .unified_sidebar
        {
            return;
        }
        self.gx_store.unified_sidebar.pending_project = Some(project_id.to_string());
        self.gx_store_unified_sidebar_changed(&ChangeSummary::default(), cx);
    }

    /// The header's New session button: the New Thread picker (agents with their accounts, then
    /// Browser and Terminal), the one the New Thread hotkey opens. The page has no picker window,
    /// so there it makes a terminal session as before.
    ///
    /// CDXC:Sidebar 2026-10-30 DECISION:
    /// User: the Unified header's New session button opens the app's existing new-session picker. It launches into the active project, as from its hotkey; when the filter narrows to one project (one repository) and the active project is not in it, that repository's project on this computer is made active first, so the session starts where the list is looking. Any other filter, or a repository only another machine has, leaves the active project as it is.
    pub(crate) fn open_unified_new_session(&mut self, cx: &mut gpui::Context<Self>) {
        let host = &self.gx_store.unified_sidebar;
        let target = match (host.filter.repos.len(), host.filter.repos.first()) {
            (1, Some(repo_key)) => {
                let active_repo = self
                    .gpui_app_modal_active_project_id()
                    .and_then(|project_id| host.repo_key_of_project(&project_id));
                (active_repo.as_deref() != Some(repo_key.as_str()))
                    .then(|| host.local_project_in_repo(repo_key))
                    .flatten()
            }
            _ => None,
        };
        if let Some(project_id) = target {
            let _ = self.dispatch_gpui_menu_bar_project_activation(&project_id, cx);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.open_gpui_new_thread_picker(cx);
        #[cfg(target_arch = "wasm32")]
        self.dispatch_native_sidebar_command(serde_json::json!({"type": "createSession"}), cx);
    }

    /// The Unified header's filter menu and its shelves. Returns whether the command was one.
    pub(crate) fn gx_store_run_unified_sidebar_command(
        &mut self,
        command: &Value,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        match command["type"].as_str() {
            Some("unifiedFilter") => {
                self.gx_store.unified_sidebar.pending_project = None;
                let filter = &mut self.gx_store.unified_sidebar.filter;
                match (command["kind"].as_str(), command["key"].as_str()) {
                    (Some("clear"), _) => *filter = UnifiedFilter::default(),
                    (Some(kind), Some(key)) => {
                        let set = match kind {
                            "repo" => &mut filter.repos,
                            "machine" => &mut filter.machines,
                            "space" => &mut filter.spaces,
                            _ => return true,
                        };
                        if !set.remove(key) {
                            set.insert(key.to_string());
                        }
                    }
                    _ => return true,
                }
                self.gx_store_unified_sidebar_changed(&ChangeSummary::default(), cx);
                true
            }
            _ => false,
        }
    }
}

/// The project's own icon, as its Project sidebar header draws it: the one set for it, else the
/// one found in its folder.
fn project_icon(project_context: Option<&Value>) -> Option<Arc<gpui::Image>> {
    project_context
        .and_then(|project| {
            project
                .get("iconDataUrl")
                .or_else(|| project.get("discoveredIconDataUrl"))
        })
        .and_then(Value::as_str)
        .and_then(crate::app::native_sidebar::images::sidebar_image)
}
