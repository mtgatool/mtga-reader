//! Shared cached-reader session (Windows/Mono).
//!
//! Holds the process handle plus the (expensive-to-find) WrapperController
//! class address so repeated typed reads skip the assembly scan. Used by both
//! the NAPI bindings and plain Rust consumers (e.g. the Tauri desktop app).

use std::sync::Mutex;

use serde_json::Value;

use crate::mono_reader::MonoReader;

/// Cached session: the reader plus the WrapperController class address.
pub struct ReaderWrapper(pub Option<MonoReader>, pub usize);
unsafe impl Send for ReaderWrapper {}
unsafe impl Sync for ReaderWrapper {}

pub static READER: Mutex<ReaderWrapper> = Mutex::new(ReaderWrapper(None, 0));

/// Open the process and pre-cache the WrapperController class so subsequent
/// typed reads skip the (expensive) assembly scan.
pub fn init(process_name: &str) -> Result<bool, String> {
    let pid = MonoReader::find_pid_by_name(process_name).ok_or("Process not found")?;

    let mut mono_reader = MonoReader::new(pid.as_u32()).map_err(|e| {
        format!(
            "Failed to open process (are you running elevated?): {}",
            e
        )
    })?;
    let mono_root = mono_reader.read_mono_root_domain();

    if mono_root == 0 {
        return Err("Failed to read mono root domain".to_string());
    }

    mono_reader.read_assembly_image();
    let wc = crate::queries::find_wrapper_controller(&mut mono_reader).unwrap_or(0);

    let mut wrapper = READER.lock().map_err(|_| "Failed to lock reader")?;
    *wrapper = ReaderWrapper(Some(mono_reader), wc);

    Ok(true)
}

pub fn close() -> Result<bool, String> {
    let mut wrapper = READER.lock().map_err(|_| "Failed to lock reader")?;
    *wrapper = ReaderWrapper(None, 0);
    Ok(true)
}

pub fn is_initialized() -> bool {
    if let Ok(wrapper) = READER.lock() {
        wrapper.0.is_some()
    } else {
        false
    }
}

/// Read from the cached session if the WrapperController instance is live.
fn try_session<F>(from: &F) -> Option<Value>
where
    F: Fn(&MonoReader, usize) -> Value,
{
    let guard = READER.lock().ok()?;
    let reader = guard.0.as_ref()?;
    if guard.1 == 0 {
        return None;
    }
    let inst = crate::queries::wrapper_instance(reader, guard.1)?;
    Some(from(reader, inst))
}

/// Run a typed reader against the cached session if init() was called and the
/// WrapperController instance is live; otherwise fall back to a fresh read.
/// When the cached session points at a dead process (game restarted), re-init
/// so subsequent reads stay fast instead of paying a full scan on every read.
fn session_or_fresh<F>(
    process_name: &str,
    from: F,
    fresh: fn(String) -> Value,
) -> Value
where
    F: Fn(&MonoReader, usize) -> Value,
{
    if let Some(v) = try_session(&from) {
        return v;
    }

    let cached_pid = READER
        .lock()
        .ok()
        .and_then(|guard| guard.0.as_ref().map(|r| r.pid()));

    if let Some(old_pid) = cached_pid {
        let live_pid = MonoReader::find_pid_by_name(process_name).map(|p| p.as_u32());
        if live_pid != Some(old_pid) && init(process_name).is_ok() {
            if let Some(v) = try_session(&from) {
                return v;
            }
        }
    }

    fresh(process_name.to_string())
}

pub fn read_decks(process_name: &str) -> Value {
    session_or_fresh(process_name, crate::queries::decks_from, crate::read_decks)
}

pub fn read_ranks(process_name: &str) -> Value {
    session_or_fresh(process_name, crate::queries::ranks_from, crate::read_ranks)
}

pub fn read_account(process_name: &str) -> Value {
    session_or_fresh(process_name, crate::queries::account_from, crate::read_account)
}

pub fn read_collection(process_name: &str) -> Value {
    session_or_fresh(process_name, crate::queries::collection_from, crate::read_collection)
}

pub fn read_inventory(process_name: &str) -> Value {
    session_or_fresh(process_name, crate::queries::inventory_from, crate::read_inventory)
}
