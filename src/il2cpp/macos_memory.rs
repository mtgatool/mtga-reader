//! macOS memory reading implementation
//!
//! Uses mach APIs to read memory from a target process.

#[cfg(target_os = "macos")]
use mach2::kern_return::KERN_SUCCESS;
#[cfg(target_os = "macos")]
use mach2::mach_types::task_t;
#[cfg(target_os = "macos")]
use mach2::port::mach_port_t;
#[cfg(target_os = "macos")]
use mach2::traps::task_for_pid;
#[cfg(target_os = "macos")]
use mach2::vm::mach_vm_read_overwrite;
#[cfg(target_os = "macos")]
use mach2::traps::mach_task_self;

use crate::backend::BackendError;

/// macOS process memory reader using mach APIs
#[cfg(target_os = "macos")]
pub struct MacOsMemoryReader {
    task: task_t,
    pid: u32,
}

#[cfg(target_os = "macos")]
impl MacOsMemoryReader {
    /// Create a new memory reader for the given process
    pub fn new(pid: u32) -> Result<Self, BackendError> {
        let mut task: mach_port_t = 0;

        let result = unsafe {
            task_for_pid(mach_task_self(), pid as i32, &mut task)
        };

        if result != KERN_SUCCESS {
            return Err(BackendError::InitializationFailed(format!(
                "task_for_pid failed with error {}. Make sure the process exists and you have the required entitlements (or run with sudo).",
                result
            )));
        }

        Ok(MacOsMemoryReader { task, pid })
    }

    /// Get the process ID
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Read bytes from the target process memory
    pub fn read_bytes(&self, addr: usize, len: usize) -> Vec<u8> {
        let mut buffer = vec![0u8; len];
        let mut size: u64 = 0;

        let result = unsafe {
            mach_vm_read_overwrite(
                self.task,
                addr as u64,
                len as u64,
                buffer.as_mut_ptr() as u64,
                &mut size,
            )
        };

        if result != KERN_SUCCESS {
            return vec![0u8; len];
        }

        buffer
    }

    /// Read a u8 from the target process
    pub fn read_u8(&self, addr: usize) -> u8 {
        let bytes = self.read_bytes(addr, 1);
        bytes[0]
    }

    /// Read a u16 from the target process
    pub fn read_u16(&self, addr: usize) -> u16 {
        let bytes = self.read_bytes(addr, 2);
        u16::from_le_bytes([bytes[0], bytes[1]])
    }

    /// Read a u32 from the target process
    pub fn read_u32(&self, addr: usize) -> u32 {
        let bytes = self.read_bytes(addr, 4);
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// Read a u64 from the target process
    pub fn read_u64(&self, addr: usize) -> u64 {
        let bytes = self.read_bytes(addr, 8);
        u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    }

    /// Read an i32 from the target process
    pub fn read_i32(&self, addr: usize) -> i32 {
        let bytes = self.read_bytes(addr, 4);
        i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// Read an i64 from the target process
    pub fn read_i64(&self, addr: usize) -> i64 {
        let bytes = self.read_bytes(addr, 8);
        i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    }

    /// Read a pointer (usize) from the target process
    pub fn read_ptr(&self, addr: usize) -> usize {
        self.read_u64(addr) as usize
    }

    /// Read a null-terminated ASCII string
    pub fn read_ascii_string(&self, addr: usize) -> Option<String> {
        if addr == 0 {
            return None;
        }

        let mut result = String::new();
        let mut offset = 0;

        loop {
            let byte = self.read_u8(addr + offset);
            if byte == 0 || offset > 1024 {
                break;
            }
            result.push(byte as char);
            offset += 1;
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Read a UTF-16 string (as used in .NET)
    pub fn read_managed_string(&self, addr: usize, length_offset: usize, chars_offset: usize) -> Option<String> {
        if addr == 0 || addr < 0x10000 {
            return None;
        }

        let length = self.read_u32(addr + length_offset);
        if length == 0 || length > 10000 {
            return None;
        }

        let mut utf16_chars = Vec::new();
        let chars_addr = addr + chars_offset;

        for i in 0..length {
            let char_val = self.read_u16(chars_addr + (i as usize * 2));
            utf16_chars.push(char_val);
        }

        String::from_utf16(&utf16_chars).ok()
    }
}

/// Find loaded libraries in a macOS process
#[cfg(target_os = "macos")]
pub fn find_library_base(_pid: u32, _library_name: &str) -> Option<usize> {
    // libproc's regionfilename requires an address, not useful for enumeration
    // We use vmmap command instead in find_game_assembly_base
    None
}

/// Get the base address of GameAssembly.dylib in the target process
#[cfg(target_os = "macos")]
pub fn find_game_assembly_base(pid: u32) -> Option<usize> {
    use std::process::Command;

    // Use vmmap to find the library base address
    // This is a workaround until we implement proper region enumeration
    let output = Command::new("vmmap")
        .args(["-wide", &pid.to_string()])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        if line.contains("GameAssembly") && line.contains("__TEXT") {
            // Parse the start address from the vmmap output
            // Format: "start-end [ size] perm/max SM=XXX path"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(addr_range) = parts.first() {
                if let Some(start_addr) = addr_range.split('-').next() {
                    if let Ok(addr) = usize::from_str_radix(start_addr.trim_start_matches("0x"), 16) {
                        return Some(addr);
                    }
                }
            }
        }
    }

    None
}

// Stub implementations for non-macOS platforms
#[cfg(not(target_os = "macos"))]
pub struct MacOsMemoryReader;

#[cfg(not(target_os = "macos"))]
impl MacOsMemoryReader {
    pub fn new(_pid: u32) -> Result<Self, BackendError> {
        Err(BackendError::InitializationFailed("macOS memory reading not available on this platform".to_string()))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn find_game_assembly_base(_pid: u32) -> Option<usize> {
    None
}
