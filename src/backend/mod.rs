//! Backend abstraction for Mono and IL2CPP runtimes

pub mod traits;
pub mod detection;

pub use traits::*;
pub use detection::{RuntimeType, detect_runtime, create_backend};
