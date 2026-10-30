//! The session's resource index: every question form, page and chart its conversation held, kept
//! in the `session_chat_resources` table and sent to the chat as `resourceIndex` on its frames, so
//! the Resources pill and list cover the conversation and not only the rows the chat has loaded.
//!
//! The index is fed by the open-questions ledger (`session_chat_open_questions.rs`), which already
//! reads each session's transcript incrementally off every UI path, a sweep every few seconds and
//! each turn end; it is never read on a UI path. A session's first read of a long transcript is the
//! backfill: it runs on a background thread, and a stopped thread of a coordinator that has no
//! index yet is backfilled the same way (`backfill_threads`). Entries are only added or updated,
//! never dropped when a transcript is compacted or rewritten.
//!
//! A coordinator's index also lists the pages its threads posted, resolved threads included
//! (`ThreadRef`), read from each thread's own stored index.
//!
//! The user's decision and the design are on gx-chat-core's questions/resource_index.rs.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use ghostex_gx_chat_core::questions::resource_index::{merge, ResourceEntry, ThreadRef};
use rusqlite::OptionalExtension;
use serde_json::Value;

use crate::domain::DomainRepository;
use crate::server::AppState;

/// How many entries a session's index keeps at most; the oldest are dropped first.
const ENTRY_LIMIT: usize = 2_000;

/// Bumped by every write to the table, so a frame's snapshot knows its cached index is stale and
/// the phone's long poll sees the change (`SessionChatQueueSnapshot::revision`).
static EPOCH: AtomicU64 = AtomicU64::new(1);

pub(crate) fn epoch() -> u64 {
    EPOCH.load(Ordering::Relaxed)
}

/// Frames read the index for each snapshot; the composed index (a coordinator's own entries plus
/// its threads' pages) is cached until the next write anywhere.
fn cache() -> &'static Mutex<HashMap<(String, String), (u64, Arc<Vec<Value>>)>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, String), (u64, Arc<Vec<Value>>)>>> =
        OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// A session's stored entries, oldest first.
pub(crate) fn read_entries(
    db: &rusqlite::Connection,
    project_id: &str,
    session_id: &str,
) -> Vec<ResourceEntry> {
    let text: Option<String> = db
        .query_row(
            "SELECT entriesJson FROM session_chat_resources WHERE projectId = ?1 AND sessionId = ?2",
            rusqlite::params![project_id, session_id],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten();
    text.and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Whether the session has an index row at all (even an empty one: it was backfilled).
pub(crate) fn has_row(db: &rusqlite::Connection, project_id: &str, session_id: &str) -> bool {
    db.query_row(
        "SELECT 1 FROM session_chat_resources WHERE projectId = ?1 AND sessionId = ?2",
        rusqlite::params![project_id, session_id],
        |_| Ok(()),
    )
    .optional()
    .ok()
    .flatten()
    .is_some()
}

fn write_entries(
    db: &rusqlite::Connection,
    project_id: &str,
    session_id: &str,
    entries: &[ResourceEntry],
) -> anyhow::Result<()> {
    let pages: Vec<&ResourceEntry> = entries
        .iter()
        .filter(|entry| entry.kind == "page" && entry.url.is_some())
        .collect();
    db.execute(
        r#"INSERT INTO session_chat_resources (projectId, sessionId, entriesJson, pagesJson, entryCount, updatedAt)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(projectId, sessionId) DO UPDATE SET
          entriesJson = excluded.entriesJson, pagesJson = excluded.pagesJson,
          entryCount = excluded.entryCount, updatedAt = excluded.updatedAt"#,
        rusqlite::params![
            project_id,
            session_id,
            serde_json::to_string(entries)?,
            serde_json::to_string(&pages)?,
            entries.len() as i64,
            chrono::Utc::now().to_rfc3339(),
        ],
    )?;
    EPOCH.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Merges what the transcript was just read to hold into the session's stored index (`stored` is
/// the ledger's copy of the table, loaded on first use). Writes the table only when something
/// changed; true when it did.
fn merge_and_write(
    db: &rusqlite::Connection,
    project_id: &str,
    session_id: &str,
    stored: &mut Option<Vec<ResourceEntry>>,
    read: Vec<ResourceEntry>,
) -> bool {
    let first = stored.is_none();
    let known = stored.get_or_insert_with(|| read_entries(db, project_id, session_id));
    let merged = merge(known.clone(), read, ENTRY_LIMIT);
    let unchanged = *known == merged;
    // The first read of a session also writes an empty index, so "backfilled, nothing found" is
    // told apart from "never read" (`has_row`).
    if unchanged && !(first && !has_row(db, project_id, session_id)) {
        return false;
    }
    *known = merged;
    write_entries(db, project_id, session_id, known).is_ok()
}

/// Brings the session's stored index up to date with what its transcript was just read to hold,
/// and tells the session's clients (and its coordinator's, for a thread's pages) when it changed.
pub(crate) fn store(
    state: &AppState,
    db: &rusqlite::Connection,
    project_id: &str,
    session_id: &str,
    stored: &mut Option<Vec<ResourceEntry>>,
    read: Vec<ResourceEntry>,
) {
    if merge_and_write(db, project_id, session_id, stored, read) {
        announce(state, db, project_id, session_id);
    }
}

/// Pushes a state frame to the session's open chats, and to its coordinator's when it is a thread
/// (its pages are listed there).
fn announce(state: &AppState, db: &rusqlite::Connection, project_id: &str, session_id: &str) {
    let repository = DomainRepository::new(db, state.metadata.server_id.as_str());
    let mut targets = vec![(project_id.to_string(), session_id.to_string())];
    if let Ok(Some(thread)) = crate::coordinators::read_thread(db, project_id, session_id) {
        targets.push(thread.coordinator_key());
    }
    for (project, session) in targets {
        if let Ok(Some(session)) = repository.get_session(&project, &session) {
            crate::session_chat_interactive::emit_session_chat_prompt_state_frame(state, &session);
        }
    }
}

/// The index a session's chat is sent: its own entries and, for a coordinator, the pages each of
/// its threads posted (resolved threads too), as JSON records.
pub(crate) fn frame_index(
    db: &rusqlite::Connection,
    project_id: &str,
    session_id: &str,
) -> Arc<Vec<Value>> {
    let epoch = epoch();
    let key = (project_id.to_string(), session_id.to_string());
    if let Some((at, index)) = cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key).cloned())
    {
        if at == epoch {
            return index;
        }
    }
    let mut entries: Vec<Value> = read_entries(db, project_id, session_id)
        .iter()
        .filter_map(|entry| serde_json::to_value(entry).ok())
        .collect();
    entries.extend(thread_pages(db, project_id, session_id));
    let index = Arc::new(entries);
    if let Ok(mut cache) = cache().lock() {
        cache.insert(key, (epoch, index.clone()));
    }
    index
}

/// The pages the threads of coordinator `project_id`/`session_id` posted, each tagged with its
/// thread. Empty for a session that is no coordinator.
fn thread_pages(db: &rusqlite::Connection, project_id: &str, session_id: &str) -> Vec<Value> {
    let Ok(threads) = crate::coordinators::list_threads_for(db, project_id, session_id) else {
        return Vec::new();
    };
    let repository = DomainRepository::new(db, "");
    let mut pages = Vec::new();
    for thread in threads {
        let text: Option<String> = db
            .query_row(
                "SELECT pagesJson FROM session_chat_resources WHERE projectId = ?1 AND sessionId = ?2",
                rusqlite::params![thread.project_id, thread.session_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten();
        let Some(text) = text else {
            continue;
        };
        let Ok(entries) = serde_json::from_str::<Vec<ResourceEntry>>(&text) else {
            continue;
        };
        if entries.is_empty() {
            continue;
        }
        let title = repository
            .get_session(&thread.project_id, &thread.session_id)
            .ok()
            .flatten()
            .map(|session| crate::coordinators::session_title_of(&session))
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| "Thread".to_string());
        let reference = ThreadRef {
            title,
            project_id: thread.project_id.clone(),
            session_id: thread.session_id.clone(),
        };
        for entry in entries {
            let entry = ResourceEntry {
                thread: Some(reference.clone()),
                ..entry
            };
            if let Ok(value) = serde_json::to_value(&entry) {
                pages.push(value);
            }
        }
    }
    pages
}

/// When each session was last checked for a missing index, so a long poll that reads the chat every
/// half second asks the table about it only now and then.
fn last_checked() -> &'static Mutex<HashMap<(String, String), std::time::Instant>> {
    static CHECKED: OnceLock<Mutex<HashMap<(String, String), std::time::Instant>>> =
        OnceLock::new();
    CHECKED.get_or_init(Default::default)
}

/// Makes sure a session that is being read has an index, and so do the threads of a coordinator
/// being read: a stopped session is never swept, and a chat opened on one must not wait for the
/// next time it runs. The reads happen on background threads; the chat is sent the index in the
/// next frame (`store` announces it).
pub(crate) fn ensure_indexed(
    state: &AppState,
    db: &rusqlite::Connection,
    project_id: &str,
    session_id: &str,
) {
    const RECHECK: std::time::Duration = std::time::Duration::from_secs(30);
    let key = (project_id.to_string(), session_id.to_string());
    if let Ok(mut checked) = last_checked().lock() {
        if checked.get(&key).is_some_and(|at| at.elapsed() < RECHECK) {
            return;
        }
        checked.insert(key.clone(), std::time::Instant::now());
    }
    if !has_row(db, project_id, session_id) {
        start_index(state, project_id, session_id);
    }
    if crate::coordinators::read_coordinator(db, project_id, session_id)
        .ok()
        .flatten()
        .is_some()
    {
        backfill_threads(state, db, project_id, session_id);
    }
}

fn start_index(state: &AppState, project_id: &str, session_id: &str) {
    let key = (project_id.to_string(), session_id.to_string());
    let started = in_flight()
        .lock()
        .map(|mut flying| flying.insert(key.clone()))
        .unwrap_or(false);
    if !started {
        return;
    }
    let state = state.clone();
    std::thread::spawn(move || {
        crate::session_chat_open_questions::index_only(&state, &key.0, &key.1);
        if let Ok(mut flying) = in_flight().lock() {
            flying.remove(&key);
        }
    });
}

/// Threads being backfilled right now, so a sweep never starts the same read twice.
fn in_flight() -> &'static Mutex<HashSet<(String, String)>> {
    static IN_FLIGHT: OnceLock<Mutex<HashSet<(String, String)>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(Default::default)
}

/// Builds the index of every thread of this coordinator that has none yet (a resolved or stopped
/// thread is never swept), once, on background threads.
pub(crate) fn backfill_threads(
    state: &AppState,
    db: &rusqlite::Connection,
    project_id: &str,
    session_id: &str,
) {
    let Ok(threads) = crate::coordinators::list_threads_for(db, project_id, session_id) else {
        return;
    };
    for thread in threads {
        if has_row(db, &thread.project_id, &thread.session_id) {
            continue;
        }
        start_index(state, &thread.project_id, &thread.session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::get_gxserver_paths;
    use crate::storage::{initialize_gxserver_storage, open_gxserver_database};

    fn database() -> (tempfile::TempDir, crate::storage::GxserverDatabase) {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = get_gxserver_paths(Some(temp.path().to_path_buf()));
        initialize_gxserver_storage(&paths).expect("storage init");
        let db = open_gxserver_database(&paths).expect("open db");
        (temp, db)
    }

    fn entry(id: &str, kind: &str, status: &str, at: i64) -> ResourceEntry {
        ResourceEntry {
            id: id.to_string(),
            kind: kind.to_string(),
            title: format!("About {id}"),
            status: status.to_string(),
            at_ms: Some(at),
            url: (kind == "page").then(|| format!("http://127.0.0.1:1/visual/{id}")),
            ..ResourceEntry::default()
        }
    }

    /// The first read of a session writes its index even when it holds nothing, so a backfill is
    /// never repeated; a read that finds nothing new writes nothing.
    #[test]
    fn the_first_read_marks_the_session_backfilled_and_later_reads_write_only_changes() {
        let (_temp, db) = database();
        assert!(!has_row(&db, "p", "s"));
        let mut stored = None;
        assert!(merge_and_write(&db, "p", "s", &mut stored, Vec::new()));
        assert!(has_row(&db, "p", "s"));
        assert!(
            !merge_and_write(&db, "p", "s", &mut stored, Vec::new()),
            "a read that finds nothing new writes nothing"
        );
        let found = vec![entry("a", "form", "waiting", 1)];
        assert!(merge_and_write(&db, "p", "s", &mut stored, found.clone()));
        assert!(!merge_and_write(&db, "p", "s", &mut stored, found));
        assert_eq!(read_entries(&db, "p", "s").len(), 1);
    }

    /// A transcript that was compacted or rewritten no longer holds the old entries; the index
    /// still does, and the count survives a restart (a new ledger loads the table).
    #[test]
    fn the_index_survives_a_compaction_and_a_restart() {
        let (_temp, db) = database();
        let mut stored = None;
        let before: Vec<ResourceEntry> = (0..300)
            .map(|n| {
                entry(
                    &format!("r{n}"),
                    if n % 5 == 0 { "page" } else { "form" },
                    "answered",
                    n,
                )
            })
            .collect();
        merge_and_write(&db, "p", "s", &mut stored, before);
        // Compaction: the rewritten transcript holds only the newest 40.
        let after: Vec<ResourceEntry> = (260..300)
            .map(|n| {
                entry(
                    &format!("r{n}"),
                    if n % 5 == 0 { "page" } else { "form" },
                    "answered",
                    n,
                )
            })
            .collect();
        merge_and_write(&db, "p", "s", &mut stored, after);
        assert_eq!(
            read_entries(&db, "p", "s").len(),
            300,
            "nothing was dropped"
        );
        // A restart: a fresh ledger (nothing cached) reads the same table, and a status that moved
        // is updated in place.
        let mut restarted = None;
        let moved = vec![entry("r299", "form", "withdrawn", 299)];
        merge_and_write(&db, "p", "s", &mut restarted, moved);
        let entries = read_entries(&db, "p", "s");
        assert_eq!(entries.len(), 300);
        assert_eq!(entries.last().map(|e| e.status.as_str()), Some("withdrawn"));
    }

    /// A coordinator's frame lists its own entries and the pages (only pages) of every thread it
    /// has, resolved ones included, each with the thread it came from.
    #[test]
    fn a_coordinators_index_adds_the_pages_its_threads_posted() {
        let (_temp, db) = database();
        crate::coordinators::link_thread(&db, "p", "thread-a", "p", "coord", "task a").unwrap();
        crate::coordinators::link_thread(&db, "p", "thread-b", "p", "coord", "task b").unwrap();
        crate::coordinators::set_thread_resolved(&db, "p", "thread-b", true).unwrap();
        crate::coordinators::link_thread(&db, "p", "stranger", "p", "other-coord", "").unwrap();
        let mut none = None;
        merge_and_write(
            &db,
            "p",
            "coord",
            &mut none,
            vec![entry("mine", "form", "answered", 1)],
        );
        let mut a = None;
        merge_and_write(
            &db,
            "p",
            "thread-a",
            &mut a,
            vec![
                entry("a-page", "page", "other", 2),
                entry("a-form", "form", "waiting", 3),
                entry("a-chart", "chart", "other", 4),
            ],
        );
        let mut b = None;
        merge_and_write(
            &db,
            "p",
            "thread-b",
            &mut b,
            vec![entry("b-page", "page", "other", 5)],
        );
        let mut other = None;
        merge_and_write(
            &db,
            "p",
            "stranger",
            &mut other,
            vec![entry("x-page", "page", "other", 6)],
        );

        let index = frame_index(&db, "p", "coord");
        let ids: Vec<&str> = index.iter().filter_map(|v| v["id"].as_str()).collect();
        assert_eq!(
            ids,
            ["mine", "a-page", "b-page"],
            "own entries, then thread pages only"
        );
        let thread_of = |id: &str| {
            index
                .iter()
                .find(|v| v["id"] == id)
                .and_then(|v| v["thread"]["sessionId"].as_str())
                .map(str::to_string)
        };
        assert_eq!(thread_of("a-page").as_deref(), Some("thread-a"));
        assert_eq!(thread_of("b-page").as_deref(), Some("thread-b"));
        assert_eq!(thread_of("mine"), None);
        // A thread's own chat lists its own entries and nothing else.
        assert_eq!(frame_index(&db, "p", "thread-a").len(), 3);
        assert_eq!(
            *frame_index(&db, "p", "coord"),
            *index,
            "read again, the same list"
        );
        let mut c = None;
        merge_and_write(
            &db,
            "p",
            "thread-a",
            &mut c,
            vec![entry("a-page-2", "page", "other", 7)],
        );
        assert_eq!(frame_index(&db, "p", "coord").len(), 4);
    }
}
