//! IL2CPP runtime backend implementation
//!
//! This module contains the IL2CPP-specific implementation for reading Unity game memory.
//! IL2CPP is used by Unity on macOS, iOS, consoles, and increasingly other platforms.

pub mod reader;
pub mod offsets;
pub mod metadata;
pub mod type_definition;
pub mod field_definition;
pub mod macho_reader;
pub mod macos_memory;

pub use reader::Il2CppBackend;
pub use offsets::Il2CppOffsets;
pub use metadata::MetadataParser;

#[cfg(target_os = "macos")]
pub use macos_memory::MacOsMemoryReader;
