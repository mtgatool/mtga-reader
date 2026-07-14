//! Public Rust API for mtga-reader
//!
//! This module provides a simple Rust API that can be used from both Tauri and other Rust applications.

use serde_json::Value as JsonValue;

/// Check if the current process has admin/elevated privileges
pub fn is_admin() -> bool {
    #[cfg(target_os = "windows")]
    {
        crate::mono_reader::MonoReader::is_admin()
    }

    #[cfg(target_os = "macos")]
    {
        unsafe { libc::geteuid() == 0 }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        false
    }
}

/// Find a process by name (returns true if found)
pub fn find_process(process_name: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        crate::mono_reader::MonoReader::find_pid_by_name(process_name).is_some()
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let output = Command::new("pgrep")
            .arg(process_name)
            .output();

        if let Ok(output) = output {
            !output.stdout.is_empty()
        } else {
            false
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = process_name;
        false
    }
}

/// Read data from process memory following a field path
#[cfg(target_os = "windows")]
pub fn read_data(process_name: &str, fields: Vec<String>) -> JsonValue {
    crate::read_data(process_name.to_string(), fields)
}

#[cfg(target_os = "macos")]
pub fn read_data(process_name: &str, fields: Vec<String>) -> JsonValue {
    let _ = (process_name, fields);
    serde_json::json!({ "error": "macOS support requires IL2CPP backend - not yet implemented in public API" })
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn read_data(process_name: &str, fields: Vec<String>) -> JsonValue {
    let _ = (process_name, fields);
    serde_json::json!({ "error": "Platform not supported" })
}

/// Read a class instance at a specific address
#[cfg(target_os = "windows")]
pub fn read_class(process_name: &str, address: i64) -> JsonValue {
    crate::read_class(process_name.to_string(), address)
}

#[cfg(not(target_os = "windows"))]
pub fn read_class(process_name: &str, address: i64) -> JsonValue {
    let _ = (process_name, address);
    serde_json::json!({ "error": "Platform not supported" })
}

/// Read a generic instance at a specific address
#[cfg(target_os = "windows")]
pub fn read_generic_instance(process_name: &str, address: i64) -> JsonValue {
    crate::read_generic_instance(process_name.to_string(), address)
}

#[cfg(not(target_os = "windows"))]
pub fn read_generic_instance(process_name: &str, address: i64) -> JsonValue {
    let _ = (process_name, address);
    serde_json::json!({ "error": "Platform not supported" })
}

// ============================================================================
// Cached session + high-level typed readers (Windows/Mono only for now)
// ============================================================================

/// Initialize a cached reader session for the typed readers. Scans assemblies
/// once and caches the WrapperController class address so subsequent reads
/// take ~10-20ms instead of a full ~4s assembly scan.
#[cfg(target_os = "windows")]
pub fn init(process_name: &str) -> Result<bool, String> {
    crate::session::init(process_name)
}

#[cfg(not(target_os = "windows"))]
pub fn init(process_name: &str) -> Result<bool, String> {
    let _ = process_name;
    Err("Platform not supported".to_string())
}

/// Clear the cached reader session.
#[cfg(target_os = "windows")]
pub fn close() -> Result<bool, String> {
    crate::session::close()
}

#[cfg(not(target_os = "windows"))]
pub fn close() -> Result<bool, String> {
    Ok(false)
}

/// Whether a cached reader session is active.
#[cfg(target_os = "windows")]
pub fn is_initialized() -> bool {
    crate::session::is_initialized()
}

#[cfg(not(target_os = "windows"))]
pub fn is_initialized() -> bool {
    false
}

macro_rules! typed_reader {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[cfg(target_os = "windows")]
        pub fn $name(process_name: &str) -> JsonValue {
            crate::session::$name(process_name)
        }

        $(#[$doc])*
        #[cfg(not(target_os = "windows"))]
        pub fn $name(process_name: &str) -> JsonValue {
            let _ = process_name;
            serde_json::json!({ "error": concat!(stringify!($name), " is not supported on this platform yet") })
        }
    };
}

typed_reader! {
    /// Read the saved decks (names, attributes, piles of {grpId, qty}).
    read_decks
}
typed_reader! {
    /// Read constructed/limited rank info for the logged-in player.
    read_ranks
}
typed_reader! {
    /// Read account identity (displayName, accountId, personaId, ...).
    read_account
}
typed_reader! {
    /// Read the card collection as {count, cards: [{grpId, qty}]}.
    read_collection
}
typed_reader! {
    /// Read inventory (gems, gold, wildcards, vault progress, ...).
    read_inventory
}
