//! Runtime detection for Mono vs IL2CPP

use sysinfo::{Pid, System};

/// The type of Unity runtime detected
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeType {
    /// Mono runtime (used on Windows, some Linux)
    Mono,
    /// IL2CPP runtime (used on macOS, iOS, consoles)
    Il2Cpp,
    /// Could not determine runtime type
    Unknown,
}

impl std::fmt::Display for RuntimeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeType::Mono => write!(f, "Mono"),
            RuntimeType::Il2Cpp => write!(f, "IL2CPP"),
            RuntimeType::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Detect the runtime type for a given process ID
///
/// Detection strategy:
/// - Windows: Check for mono-2.0-bdwgc.dll (Mono) or GameAssembly.dll (IL2CPP)
/// - Linux: Check for libmono*.so (Mono) or GameAssembly.so (IL2CPP)
/// - macOS: Check for Mono.framework (Mono) or GameAssembly.dylib (IL2CPP)
#[allow(unused_variables)]
pub fn detect_runtime(pid: u32) -> RuntimeType {
    #[cfg(target_os = "windows")]
    {
        detect_runtime_windows(pid)
    }

    #[cfg(target_os = "linux")]
    {
        detect_runtime_linux(pid)
    }

    #[cfg(target_os = "macos")]
    {
        detect_runtime_macos(pid)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        RuntimeType::Unknown
    }
}

#[cfg(target_os = "windows")]
fn detect_runtime_windows(pid: u32) -> RuntimeType {
    use proc_mem::Process;

    let process = match Process::with_pid(pid) {
        Ok(p) => p,
        Err(_) => return RuntimeType::Unknown,
    };

    // Check for Mono runtime
    if process.module("mono-2.0-bdwgc.dll").is_ok() {
        return RuntimeType::Mono;
    }

    // Check for IL2CPP runtime
    if process.module("GameAssembly.dll").is_ok() {
        return RuntimeType::Il2Cpp;
    }

    RuntimeType::Unknown
}

#[cfg(target_os = "linux")]
fn detect_runtime_linux(pid: u32) -> RuntimeType {
    // Read /proc/<pid>/maps to find loaded libraries
    let maps_path = format!("/proc/{}/maps", pid);
    let maps = match std::fs::read_to_string(&maps_path) {
        Ok(m) => m,
        Err(_) => return RuntimeType::Unknown,
    };

    // Check for Mono libraries
    if maps.contains("libmono") || maps.contains("mono-2.0") {
        return RuntimeType::Mono;
    }

    // Check for IL2CPP
    if maps.contains("GameAssembly.so") {
        return RuntimeType::Il2Cpp;
    }

    RuntimeType::Unknown
}

#[cfg(target_os = "macos")]
fn detect_runtime_macos(_pid: u32) -> RuntimeType {
    // On macOS, most Unity games use IL2CPP
    // We can check loaded libraries via vmmap or similar tools
    // For now, we'll default to IL2CPP for macOS since MTGA uses it

    // TODO: Implement proper detection using mach APIs to enumerate loaded images
    // This could use task_for_pid and iterate through dyld images

    // For MTGA specifically, it uses IL2CPP on macOS
    RuntimeType::Il2Cpp
}

/// Find a process by name and return its PID
pub fn find_process_by_name(name: &str) -> Option<Pid> {
    let mut sys = System::new_all();
    sys.refresh_all();

    sys.processes()
        .iter()
        .find(|(_, process)| process.name().contains(name))
        .map(|(pid, _)| *pid)
}

/// Create a backend for the given process
/// Returns the appropriate backend (Mono or IL2CPP) based on runtime detection
pub fn create_backend(pid: u32) -> Result<super::BoxedBackend, super::BackendError> {
    let runtime = detect_runtime(pid);

    match runtime {
        RuntimeType::Mono => {
            #[cfg(feature = "mono")]
            {
                // Will be implemented in mono module
                // let backend = crate::mono::MonoBackend::new(pid);
                // Ok(Box::new(backend))
                Err(super::BackendError::Other("Mono backend not yet integrated".to_string()))
            }
            #[cfg(not(feature = "mono"))]
            {
                Err(super::BackendError::Other("Mono backend not enabled".to_string()))
            }
        }
        RuntimeType::Il2Cpp => {
            #[cfg(feature = "il2cpp")]
            {
                // Will be implemented in il2cpp module
                // let backend = crate::il2cpp::Il2CppBackend::new(pid);
                // Ok(Box::new(backend))
                Err(super::BackendError::Other("IL2CPP backend not yet integrated".to_string()))
            }
            #[cfg(not(feature = "il2cpp"))]
            {
                Err(super::BackendError::Other("IL2CPP backend not enabled".to_string()))
            }
        }
        RuntimeType::Unknown => {
            Err(super::BackendError::InitializationFailed(
                "Could not detect runtime type (Mono or IL2CPP)".to_string()
            ))
        }
    }
}
