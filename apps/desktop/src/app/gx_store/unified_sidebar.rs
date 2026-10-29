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
    build_unified_sidebar, unified_machine_spaces,
};
use ghostex_gx_core::{
    ChangeSummary, Core, LOCAL_MACHINE_ID, MachineId, MenuHost, SessionKey, SidebarInputs,
    SidebarMenus, SidebarViewModel,
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
            group_working_sessions: list_inputs.settings.group_working_sessions,
            enable_session_parking: list_inputs.settings.enable_session_parking,
        };
        let has_projects = inputs.iter().any(|input| !input.view.groups.is_empty());
        let view = build_unified_sidebar(&inputs, settings, &self.filter, now_ms);
        let card = |row: UnifiedRow| -> Option<UnifiedCard> {
            Some(UnifiedCard {
                session: sessions
                    .get(&(
                        row.machine.clone(),
                        row.session.row.sidebar_session_id.clone(),
                    ))?
                    .clone(),
                group: groups
                    .get(&(row.machine.clone(), row.group.core.group_id.clone()))?
                    .clone(),
                row: row.session.row.clone(),
                repo_label: row.repo.label,
                machine_label: row.machine_label,
                status: row.status,
            })
        };
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
            list,
            shelves,
            options: view.options,
            filter: self.filter.clone(),
            total: view.total,
            has_projects,
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

    /// The Unified header's filter menu and its shelves. Returns whether the command was one.
    pub(crate) fn gx_store_run_unified_sidebar_command(
        &mut self,
        command: &Value,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        match command["type"].as_str() {
            Some("unifiedFilter") => {
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
