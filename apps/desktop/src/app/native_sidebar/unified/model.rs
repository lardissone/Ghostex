//! What the Unified sidebar draws, as the store installs it.

use std::sync::Arc;

use ghostex_gx_core::SessionRow;
use ghostex_gx_core::unified_sidebar::{
    UnifiedFilter, UnifiedFilterOptions, UnifiedShelf, UnifiedStatus,
};

use crate::app::native_sidebar::model::NativeSidebarSession;

/// One session card.
#[derive(Clone, Debug)]
pub(crate) struct UnifiedCard {
    /// The classic row of the same session, whose menu, hover buttons and agent icon the card
    /// reuses.
    pub(crate) session: Arc<NativeSidebarSession>,
    pub(crate) row: Arc<SessionRow>,
    /// The repository's icon, from whichever machine's project has one.
    pub(crate) repo_icon: Option<Arc<gpui::Image>>,
    pub(crate) repo_label: String,
    /// The machine's name for a session on another machine.
    pub(crate) machine_label: Option<String>,
    pub(crate) status: Option<UnifiedStatus>,
}

#[derive(Clone, Debug)]
pub(crate) struct UnifiedShelfView {
    pub(crate) shelf: UnifiedShelf,
    pub(crate) cards: Vec<UnifiedCard>,
}

/// The whole Unified sidebar.
#[derive(Clone, Debug, Default)]
pub(crate) struct UnifiedSidebarSnapshot {
    /// Sessions never sent a message that hold unsent text, newest first, above everything.
    pub(crate) drafts: Vec<UnifiedCard>,
    /// Pinned then the rest, in the order they last came back to the user.
    pub(crate) list: Vec<UnifiedCard>,
    /// Working, Snoozed and Parked, in that order; an empty shelf is left out.
    pub(crate) shelves: Vec<UnifiedShelfView>,
    pub(crate) options: UnifiedFilterOptions,
    pub(crate) filter: UnifiedFilter,
    /// Sessions before the filter.
    pub(crate) total: usize,
    /// Some machine has a project, so an empty list is not the first run.
    pub(crate) has_projects: bool,
    /// Enable session parking is on, so a card offers Park.
    pub(crate) parking: bool,
}

impl UnifiedSidebarSnapshot {
    pub(crate) fn is_empty(&self) -> bool {
        self.drafts.is_empty() && self.list.is_empty() && self.shelves.is_empty()
    }

    /// Whether any card shows a running timer, so the list redraws once a second.
    pub(crate) fn has_running_timer(&self) -> bool {
        self.drafts
            .iter()
            .chain(&self.list)
            .chain(self.shelves.iter().flat_map(|shelf| shelf.cards.iter()))
            .any(|card| card.status == Some(UnifiedStatus::Working))
    }
}
