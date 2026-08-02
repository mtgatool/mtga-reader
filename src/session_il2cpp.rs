//! Shared cached-session for the macOS / IL2CPP typed readers.
//!
//! Attaching is expensive (it scans GameAssembly's data segment for the
//! type-info table, ~4s), and class lookups walk a 39k-entry table, so both are
//! cached here. Only the root singleton pointer is re-read per poll, which
//! keeps repeat reads in the millisecond range.

#![cfg(target_os = "macos")]

use std::sync::Mutex;

use serde_json::{json, Value};

use crate::il2cpp::macos_runtime::{find_pid, Il2Cpp};
use crate::queries_il2cpp as q;

pub struct SessionWrapper(pub Option<Il2Cpp>);
// `Il2Cpp` holds interior-mutability caches and a mach port; access is
// serialised by the mutex below.
unsafe impl Send for SessionWrapper {}
unsafe impl Sync for SessionWrapper {}

pub static SESSION: Mutex<SessionWrapper> = Mutex::new(SessionWrapper(None));

/// Attach to the game and cache the runtime, pre-warming the root class lookup.
pub fn init(process_name: &str) -> Result<bool, String> {
    let pid = find_pid(process_name).ok_or("Process not found")?;
    let rt = Il2Cpp::attach(pid)?;

    // Warm the class cache so the first typed read isn't the one that pays for
    // the table scan.
    let _ = rt.find_class("WrapperController");

    let mut guard = SESSION.lock().map_err(|_| "Failed to lock session")?;
    guard.0 = Some(rt);
    Ok(true)
}

pub fn close() -> Result<bool, String> {
    let mut guard = SESSION.lock().map_err(|_| "Failed to lock session")?;
    guard.0 = None;
    Ok(true)
}

pub fn is_initialized() -> bool {
    SESSION.lock().map(|g| g.0.is_some()).unwrap_or(false)
}

/// Run `f` against the cached runtime without requiring the home-screen root
/// singleton. Used by `readData`, which navigates from arbitrary root classes
/// (e.g. `MatchSceneManager` during a match, when the wrapper scene is gone).
pub fn read_raw<F>(process_name: &str, f: F) -> Value
where
    F: Fn(&Il2Cpp) -> Value,
{
    if let Err(e) = ensure_attached(process_name) {
        return e;
    }

    let guard = match SESSION.lock() {
        Ok(g) => g,
        Err(_) => return json!({ "error": "failed to lock the IL2CPP session" }),
    };
    match guard.0.as_ref() {
        Some(rt) => {
            rt.mem.refresh_regions();
            f(rt)
        }
        None => json!({ "error": "IL2CPP session not initialized" }),
    }
}

/// Attach if there is no session, or the cached one points at a dead or
/// replaced process.
fn ensure_attached(process_name: &str) -> std::result::Result<(), Value> {
    let live_pid = find_pid(process_name);

    let needs_init = match SESSION.lock() {
        Ok(guard) => match guard.0.as_ref() {
            Some(rt) => Some(rt.mem.pid) != live_pid,
            None => true,
        },
        Err(_) => return Err(json!({ "error": "failed to lock the IL2CPP session" })),
    };

    if needs_init {
        if live_pid.is_none() {
            return Err(json!({ "error": format!("process '{process_name}' not found") }));
        }
        if let Err(e) = init(process_name) {
            return Err(json!({ "error": e }));
        }
    }
    Ok(())
}

/// Run a typed reader against the cached session, attaching (or re-attaching
/// after a game restart) when needed.
pub fn read_with<F>(process_name: &str, f: F) -> Value
where
    F: Fn(&Il2Cpp, usize) -> Value,
{
    if let Err(e) = ensure_attached(process_name) {
        return e;
    }

    let guard = match SESSION.lock() {
        Ok(g) => g,
        Err(_) => return json!({ "error": "failed to lock the IL2CPP session" }),
    };
    let rt = match guard.0.as_ref() {
        Some(rt) => rt,
        None => return json!({ "error": "IL2CPP session not initialized" }),
    };

    // The VM map moves as the game runs; refresh so pointer validation and the
    // page cache stay accurate.
    rt.mem.refresh_regions();

    match q::find_root(rt) {
        Some(root) => f(rt, root),
        None => json!({
            "error": "root singleton is null (must be on the home screen, not in a match)"
        }),
    }
}

pub fn read_account(process_name: &str) -> Value {
    read_with(process_name, q::account_from)
}

pub fn read_collection(process_name: &str) -> Value {
    read_with(process_name, q::collection_from)
}

pub fn read_inventory(process_name: &str) -> Value {
    read_with(process_name, q::inventory_from)
}

pub fn read_ranks(process_name: &str) -> Value {
    read_with(process_name, q::ranks_from)
}

pub fn read_decks(process_name: &str) -> Value {
    read_with(process_name, q::decks_from)
}
