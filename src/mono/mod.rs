//! Mono runtime backend implementation
//!
//! This module contains the Mono-specific implementation for reading Unity game memory.
//! Mono is used by Unity on Windows and some Linux builds.

pub mod reader;
pub mod offsets;
pub mod type_definition;
pub mod field_definition;
pub mod pe_reader;

pub use reader::MonoBackend;
pub use offsets::MonoOffsets;
