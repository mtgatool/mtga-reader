//! Mono-runtime scanner for MTG Arena on Windows (and Wine).
//!
//! Mirrors the IL2CPP scanner logic in `src/napi/mod.rs::macos_backend`
//! but uses Mono runtime metadata layouts. C# object field offsets are
//! identical across IL2CPP and Mono because both backends compile from
//! the same Unity 2022.3.62f2 source — only runtime-metadata access
//! patterns differ.

#[cfg(not(target_os = "macos"))]
use crate::mono_reader::MonoReader;
use std::process::Command;

// ──────────────────────────────────────────────────────────────────
// Cross-platform memory reader wrapper
// ──────────────────────────────────────────────────────────────────
//
// On macOS, Wine/CrossOver processes require direct mach task_for_pid
// via the mach2 crate (the `process_memory` crate used by MonoReader
// fails with KERN_FAILURE on these processes). On other platforms,
// MonoReader works fine.

#[cfg(target_os = "macos")]
pub struct MemReader {
    inner: crate::il2cpp::macos_memory::MacOsMemoryReader,
}

#[cfg(not(target_os = "macos"))]
pub struct MemReader {
    inner: MonoReader,
    pid: u32,
    #[cfg(target_os = "windows")]
    bulk_handle: *mut std::ffi::c_void,
}

#[cfg(target_os = "macos")]
impl MemReader {
    pub fn new(pid: u32) -> Result<Self, String> {
        let inner = crate::il2cpp::macos_memory::MacOsMemoryReader::new(pid)
            .map_err(|e| format!("Failed to attach to process {}: {}", pid, e))?;
        Ok(MemReader { inner })
    }

    pub fn read_ptr(&self, addr: usize) -> usize {
        self.inner.read_ptr(addr)
    }

    pub fn read_i32(&self, addr: usize) -> i32 {
        self.inner.read_i32(addr)
    }

    pub fn read_u32(&self, addr: usize) -> u32 {
        self.inner.read_u32(addr)
    }

    pub fn read_u16(&self, addr: usize) -> u16 {
        self.inner.read_u16(addr)
    }

    pub fn read_bytes(&self, addr: usize, len: usize) -> Vec<u8> {
        self.inner.read_bytes(addr, len)
    }

    pub fn read_ascii_string(&self, addr: usize) -> String {
        self.inner.read_ascii_string(addr).unwrap_or_default()
    }

    pub fn read_f64(&self, addr: usize) -> f64 {
        let bytes = self.inner.read_bytes(addr, 8);
        if bytes.len() == 8 {
            f64::from_le_bytes(bytes.try_into().unwrap())
        } else {
            0.0
        }
    }

    pub fn read_mono_string(&self, string_ptr: usize) -> Option<String> {
        if string_ptr == 0 || string_ptr < 0x10000 {
            return None;
        }
        // MonoString: vtable(8) + monitor(8) + length(4) + chars...
        // SIZE_OF_PTR is 8 on x64
        const SIZE_OF_PTR: usize = 8;
        let length = self.inner.read_u32(string_ptr + SIZE_OF_PTR * 2);
        if length == 0 || length > 10000 {
            return None;
        }
        let chars_offset = string_ptr + SIZE_OF_PTR * 2 + 4;
        let mut utf16_chars = Vec::new();
        for i in 0..length {
            let char_val = self.inner.read_u16(chars_offset + (i as usize * 2));
            utf16_chars.push(char_val);
        }
        String::from_utf16(&utf16_chars).ok()
    }
}

#[cfg(not(target_os = "macos"))]
unsafe impl Send for MemReader {}
#[cfg(not(target_os = "macos"))]
unsafe impl Sync for MemReader {}

#[cfg(not(target_os = "macos"))]
impl MemReader {
    pub fn new(pid: u32) -> Result<Self, String> {
        #[cfg(target_os = "windows")]
        let bulk_handle = unsafe {
            use winapi::um::processthreadsapi::OpenProcess;
            use winapi::um::winnt::PROCESS_VM_READ;
            let h = OpenProcess(PROCESS_VM_READ, 0, pid);
            if h.is_null() {
                return Err(format!("OpenProcess failed for pid {}", pid));
            }
            h
        };
        Ok(MemReader {
            inner: MonoReader::new(pid),
            pid,
            #[cfg(target_os = "windows")]
            bulk_handle,
        })
    }

    pub fn read_ptr(&self, addr: usize) -> usize {
        self.inner.read_ptr(addr)
    }

    pub fn read_i32(&self, addr: usize) -> i32 {
        self.inner.read_i32(addr)
    }

    pub fn read_u32(&self, addr: usize) -> u32 {
        self.inner.read_u32(addr)
    }

    pub fn read_u16(&self, addr: usize) -> u16 {
        self.inner.read_u16(addr)
    }

    /// Bulk memory read using a single ReadProcessMemory syscall.
    /// MonoReader's `read_bytes` does byte-by-byte reads (each byte =
    /// one ReadProcessMemory call), which is catastrophically slow for
    /// heap scanning. This override uses a cached process handle and
    /// reads the entire buffer in one call.
    #[cfg(target_os = "windows")]
    pub fn read_bytes(&self, addr: usize, len: usize) -> Vec<u8> {
        use winapi::um::memoryapi::ReadProcessMemory;
        if len == 0 {
            return Vec::new();
        }
        let mut buffer = vec![0u8; len];
        let mut bytes_read: usize = 0;
        let ok = unsafe {
            ReadProcessMemory(
                self.bulk_handle,
                addr as *const _,
                buffer.as_mut_ptr() as *mut _,
                len,
                &mut bytes_read as *mut usize as *mut _,
            )
        };
        if ok == 0 {
            buffer.fill(0);
        } else if bytes_read < len {
            buffer[bytes_read..].fill(0);
        }
        buffer
    }

    #[cfg(not(target_os = "windows"))]
    pub fn read_bytes(&self, addr: usize, len: usize) -> Vec<u8> {
        self.inner.read_bytes(addr, len)
    }

    pub fn read_ascii_string(&self, addr: usize) -> String {
        self.inner.read_ascii_string(addr)
    }

    pub fn read_f64(&self, addr: usize) -> f64 {
        self.inner.read_f64(addr)
    }

    pub fn read_mono_string(&self, string_ptr: usize) -> Option<String> {
        self.inner.read_mono_string(string_ptr)
    }
}

/// Pointer plausibility bounds. Wine on macOS maps the Windows process's
/// virtual memory into the low half of 64-bit address space. We accept
/// anything above 0x10000 (minimum Windows allocation granularity) and
/// below 0x7FFF_FFFF_FFFF (Windows user-mode ceiling). These are wider
/// than the macOS IL2CPP scanner's [0x1_0000_0000, 0x4_0000_0000]
/// because Wine's address layout differs from native macOS arm64.
const MIN_PTR: usize = 0x10000;
const MAX_PTR: usize = 0x7FFF_FFFF_FFFF;

/// Mono vs IL2CPP object header difference.
///
/// **Key discovery (2026-04-12):** Mono objects have an 8-byte header
/// (just `MonoVTable*`), while IL2CPP objects have a 16-byte header
/// (`Il2CppClass*` + `monitor*`). Instance fields start 8 bytes
/// earlier on Mono. This applies to ALL objects including
/// Dictionary, Array, and game-specific classes.
///
/// Concretely:
/// - Dict._buckets: IL2CPP +0x10, Mono **+0x08**
/// - Dict._entries: IL2CPP +0x18, Mono **+0x10**
/// - Dict._count:   IL2CPP +0x20, Mono **+0x18**
/// - Array elements: IL2CPP +0x20, Mono **+0x10**
///   (Mono: vtable(8) + length(8) = 16 bytes before first element)
/// - ClientPlayerInventory.wcCommon: IL2CPP +0x10, Mono **+0x08**
const MONO_OBJ_HEADER: usize = 0x08;

/// Dict/Array field offsets — SAME as IL2CPP.
/// Standalone heap objects (Dictionary, Array, etc.) have the full
/// 16-byte MonoObject header (vtable + synchronisation), identical to
/// IL2CPP's (klass + monitor). The 8-byte header applies only to
/// embedded value-type structs like the inventory inside
/// AwsInventoryServiceWrapper.
const DICT_BUCKETS: usize = 0x10;
const DICT_ENTRIES: usize = 0x18;
const DICT_COUNT: usize = 0x20;

/// Array elements: vtable(8) + sync(8) + bounds(8) + max_length(4) + pad(4) = 0x20.
const ARRAY_HEADER: usize = 0x20;

/// Inventory field offsets on Mono (8 bytes less than IL2CPP).
const INV_WC_COMMON: usize = MONO_OBJ_HEADER;        // 0x08
const INV_WC_UNCOMMON: usize = MONO_OBJ_HEADER + 4;  // 0x0C
const INV_WC_RARE: usize = MONO_OBJ_HEADER + 8;      // 0x10
const INV_WC_MYTHIC: usize = MONO_OBJ_HEADER + 12;   // 0x14
const INV_GOLD: usize = MONO_OBJ_HEADER + 16;        // 0x18
const INV_GEMS: usize = MONO_OBJ_HEADER + 20;        // 0x1C
const INV_VAULT: usize = MONO_OBJ_HEADER + 32;       // 0x28

/// MonoClass struct offsets verified on Windows Arena Unity 2022.3.62f2:
/// - name at +0x48 → "ClientPlayerInventory"
/// - namespace at +0x50 → "Wizards.Mtga.Inventory"
/// Confirmed by probing the template ClientPlayerInventory's MonoClass
/// at 0x211f37f2470 on a live Windows Arena process (2026-04-12).
mod mono_class_offsets {
    /// MonoClass.name — pointer to ASCII class name string
    pub const NAME: usize = 0x48;
    /// MonoClass.namespace — pointer to ASCII namespace string
    pub const NAMESPACE: usize = 0x50;
    /// MonoClass.fields — pointer to MonoClassField array (or inline on some forks)
    pub const FIELDS: usize = 0x98;
    /// MonoClass.field.count
    pub const FIELD_COUNT: usize = 0xE0;
}

/// Read a Mono class's name from its MonoClass pointer.
/// On Mono, an object's class is at `read_ptr(read_ptr(obj))` (obj →
/// MonoVTable → MonoClass). The caller passes the MonoClass pointer.
pub fn read_mono_class_name(reader: &MemReader, class_ptr: usize) -> String {
    if class_ptr < MIN_PTR || class_ptr > MAX_PTR {
        return String::new();
    }
    let name_ptr = reader.read_ptr(class_ptr + mono_class_offsets::NAME);
    if name_ptr < MIN_PTR || name_ptr > MAX_PTR {
        return String::new();
    }
    reader.read_ascii_string(name_ptr)
}

/// Resolve MonoClass from an object address via Mono's vtable indirection.
/// `obj[+0x00]` is a `MonoVTable*`; `vtable[+0x00]` is the `MonoClass*`.
pub fn obj_to_mono_class(reader: &MemReader, obj: usize) -> usize {
    let vtable = reader.read_ptr(obj);
    if vtable < MIN_PTR || vtable > MAX_PTR {
        return 0;
    }
    reader.read_ptr(vtable)
}

/// Field info extracted from a MonoClass's field array.
#[derive(Debug, Clone)]
pub struct MonoFieldInfo {
    pub name: String,
    pub offset: i32,
    pub is_static: bool,
}

/// Enumerate instance fields on a MonoClass.
///
/// On Unity's Mono fork, `MonoClass.fields` at offset 0x98 points to
/// (or contains inline) an array of `MonoClassField` entries. Each entry
/// is 0x20 bytes. We read up to `field_count` entries and extract
/// name + offset for each.
///
/// The field-entry internal layout is discovered empirically — the first
/// run's diagnostic dump (via `MTGA_DEBUG_MONO=1`) confirms which bytes
/// are the name pointer and which are the instance offset. The initial
/// assumption matches upstream's `FieldDefinition::new`:
///   - `entry + 0x00`: MonoType* (8 bytes)
///   - `entry + 0x08`: name_ptr  (8 bytes) — our primary target
///   - `entry + 0x10`: parent_class_ptr (8 bytes)
///   - `entry + 0x18`: offset (i32) — instance offset
///   - `entry + 0x1C`: token (u32)
/// Total stride: 0x20 = 32 bytes.
///
/// **If the first run produces garbage field names**, adjust the
/// `NAME_OFF` and `OFFSET_OFF` constants below and re-run.
pub fn mono_get_class_fields(reader: &MemReader, class_ptr: usize) -> Vec<MonoFieldInfo> {
    const STRIDE: usize = 0x20;
    const NAME_OFF: usize = 0x08;   // Offset of name_ptr within MonoClassField
    const OFFSET_OFF: usize = 0x18; // Offset of field_offset (i32) within MonoClassField
    const MAX_FIELDS: usize = 60;

    let field_count = reader.read_i32(class_ptr + mono_class_offsets::FIELD_COUNT);
    if field_count <= 0 || field_count > MAX_FIELDS as i32 {
        return Vec::new();
    }

    // Read the fields base. In Unity's Mono fork this is typically
    // a pointer at MonoClass + 0x98 that we dereference. If reading
    // as a pointer gives a valid address, use it. Otherwise try
    // inline (class_ptr + 0x98 directly, as upstream does).
    let fields_ptr_raw = reader.read_ptr(class_ptr + mono_class_offsets::FIELDS);
    let fields_base = if fields_ptr_raw >= MIN_PTR && fields_ptr_raw <= MAX_PTR {
        // Dereferenced pointer — standard Mono layout.
        fields_ptr_raw
    } else {
        // Inline fields — some Unity Mono forks store fields at class + FIELDS directly.
        class_ptr + mono_class_offsets::FIELDS
    };

    let debug = std::env::var("MTGA_DEBUG_MONO").is_ok();
    let mut result = Vec::with_capacity(field_count as usize);
    for i in 0..field_count as usize {
        let entry = fields_base + i * STRIDE;
        let name_ptr = reader.read_ptr(entry + NAME_OFF);
        if name_ptr < MIN_PTR || name_ptr > MAX_PTR {
            if debug {
                eprintln!(
                    "mono_get_class_fields: field[{}] at 0x{:x}: name_ptr=0x{:x} invalid, stopping",
                    i, entry, name_ptr,
                );
            }
            break;
        }
        let name = reader.read_ascii_string(name_ptr);
        if name.is_empty() || name.len() > 128 {
            if debug {
                eprintln!(
                    "mono_get_class_fields: field[{}] at 0x{:x}: empty/long name, stopping",
                    i, entry,
                );
            }
            break;
        }
        let offset = reader.read_i32(entry + OFFSET_OFF);

        // Determine if static by checking if the type pointer has
        // the static flag set. On Mono, MonoType at entry+0x00 has
        // attributes at +0x08, and static is bit 0x10. If the type
        // pointer is invalid, assume instance (non-static).
        let type_ptr = reader.read_ptr(entry);
        let is_static = if type_ptr >= MIN_PTR && type_ptr <= MAX_PTR {
            let attrs = reader.read_u32(type_ptr + 0x08);
            (attrs & 0x10) != 0
        } else {
            false
        };

        if debug {
            eprintln!(
                "  field[{}] {:?} @ 0x{:x} (static: {})",
                i, name, offset, is_static,
            );
        }
        result.push(MonoFieldInfo { name, offset, is_static });
    }
    result
}

/// Find all writable heap regions for the given process via `vmmap`.
/// Same logic as `macos_backend::find_scannable_heap_regions` but
/// excludes regions containing `mono-2.0-bdwgc` or `UnityPlayer`
/// instead of `GameAssembly` (since Wine Arena uses Mono, not IL2CPP).
pub fn find_scannable_heap_regions(pid: u32) -> Vec<(usize, usize)> {
    #[cfg(target_os = "macos")]
    {
        find_scannable_heap_regions_vmmap(pid)
    }
    #[cfg(target_os = "windows")]
    {
        find_scannable_heap_regions_windows(pid)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = pid;
        Vec::new()
    }
}

/// macOS: parse vmmap output for writable heap regions.
#[cfg(target_os = "macos")]
fn find_scannable_heap_regions_vmmap(pid: u32) -> Vec<(usize, usize)> {
    let output = Command::new("vmmap")
        .args(["-wide", &pid.to_string()])
        .output()
        .ok();

    let mut result: Vec<(usize, usize)> = Vec::new();
    const MIN_SIZE: usize = 1 << 20;
    const MAX_SIZE: usize = 4usize << 30;

    if let Some(output) = output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains("mono-2.0-bdwgc")
                || line.contains("UnityPlayer")
                || line.contains("GameAssembly")
            {
                continue;
            }
            if !line.contains("rw-") {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            let addr_field_idx = parts.iter().position(|p| {
                p.contains('-')
                    && p.split('-').count() == 2
                    && p.chars().next().map_or(false, |c| c.is_ascii_hexdigit())
            });
            let idx = match addr_field_idx {
                Some(i) => i,
                None => continue,
            };
            let addr_parts: Vec<&str> = parts[idx].split('-').collect();
            if addr_parts.len() != 2 {
                continue;
            }
            let start = match usize::from_str_radix(addr_parts[0], 16) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let end = match usize::from_str_radix(addr_parts[1], 16) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if end <= start {
                continue;
            }
            let size = end - start;
            if size < MIN_SIZE || size > MAX_SIZE {
                continue;
            }
            result.push((start, end));
        }
    }
    result.sort();
    result.dedup();
    result
}

/// Windows: walk virtual memory with VirtualQueryEx to find writable
/// private regions (where the GC heap lives).
#[cfg(target_os = "windows")]
fn find_scannable_heap_regions_windows(pid: u32) -> Vec<(usize, usize)> {
    use winapi::um::memoryapi::VirtualQueryEx;
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::winnt::{
        MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_PRIVATE,
        PAGE_READWRITE, PAGE_EXECUTE_READWRITE,
        PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    const MIN_SIZE: usize = 1 << 20; // 1 MB
    const MAX_SIZE: usize = 256 << 20; // 256 MB — cap to avoid multi-GB reads

    let handle = unsafe {
        OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid)
    };
    if handle.is_null() {
        let err = unsafe { winapi::um::errhandlingapi::GetLastError() };
        let debug = std::env::var("MTGA_DEBUG_MONO").is_ok();
        if debug {
            eprintln!(
                "find_scannable_heap_regions_windows: OpenProcess failed for pid {} (error {})",
                pid, err,
            );
        }
        return Vec::new();
    }

    let mut result: Vec<(usize, usize)> = Vec::new();
    let mut addr: usize = 0;
    let mbi_size = std::mem::size_of::<MEMORY_BASIC_INFORMATION>();

    loop {
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        let ret = unsafe {
            VirtualQueryEx(
                handle,
                addr as *const _,
                &mut mbi,
                mbi_size,
            )
        };
        if ret == 0 {
            break; // No more regions
        }

        let region_start = mbi.BaseAddress as usize;
        let region_size = mbi.RegionSize;
        let region_end = region_start + region_size;

        let is_committed = mbi.State == MEM_COMMIT;
        let is_writable = (mbi.Protect & PAGE_READWRITE) != 0
            || (mbi.Protect & PAGE_EXECUTE_READWRITE) != 0;
        let is_private = mbi.Type == MEM_PRIVATE;

        if is_committed
            && is_writable
            && is_private
            && region_size >= MIN_SIZE
            && region_size <= MAX_SIZE
        {
            result.push((region_start, region_end));
        }

        // Advance to next region
        addr = region_end;
        if addr <= region_start {
            break; // Overflow protection
        }
    }

    unsafe { CloseHandle(handle) };

    let debug = std::env::var("MTGA_DEBUG_MONO").is_ok();
    if debug {
        eprintln!(
            "find_scannable_heap_regions_windows: found {} writable private regions for pid {}",
            result.len(), pid,
        );
    }

    result.sort();
    result.dedup();
    result
}

// ──────────────────────────────────────────────────────────────────
// readMtgaCards — scan for the card-collection Dictionary<int, int>
// ──────────────────────────────────────────────────────────────────

/// Scan heap for the card-collection data. On Mono, we scan for the
/// Entry[] array directly (an array of 16-byte entries where
/// hash==key, key in card-ID range, value in [1,4]) rather than
/// finding the Dictionary wrapper first. This is more robust because
/// the wrapper's field offsets may vary, but the entry data layout
/// is standard .NET and confirmed via byte probing.
///
/// Returns the address of the Entry[] array object (elements at +0x20).
pub fn scan_heap_for_cards_dictionary(reader: &MemReader, pid: u32) -> usize {
    const MIN_CARD_ID: i32 = 5_000;
    const MAX_CARD_ID: i32 = 200_000;
    const MIN_QUANTITY: i32 = 1;
    const MAX_QUANTITY: i32 = 4;
    // Stride 16: hash(4) + next(4) + key(4) + value(4)
    const ENTRY_STRIDE: usize = 16;
    // Require 20 consecutive valid entries to identify the array
    const MIN_CONSECUTIVE: usize = 20;

    let debug = std::env::var("MTGA_DEBUG_MONO").is_ok();
    let heap_regions = find_scannable_heap_regions(pid);
    if debug {
        eprintln!(
            "mono::scan_heap_for_cards_dictionary: scanning {} heap regions for Entry[] pattern (stride {})",
            heap_regions.len(), ENTRY_STRIDE,
        );
    }

    // Scan for consecutive valid entries directly in the heap buffer.
    // Each valid entry has hash==key, key in card-ID range, value in
    // [1,4]. A run of 20+ consecutive valid entries is almost
    // certainly the card-collection Entry[].
    let mut best_run_start: usize = 0;
    let mut best_run_len: usize = 0;

    for (start, end) in heap_regions {
        let size = end - start;
        let buf = reader.read_bytes(start, size);
        if buf.len() != size {
            continue;
        }
        // Scan at 16-byte stride (entry-aligned)
        let max_entries = buf.len() / ENTRY_STRIDE;
        let mut run_start: usize = 0;
        let mut run_len: usize = 0;
        for entry_idx in 0..max_entries {
            let off = entry_idx * ENTRY_STRIDE;
            if off + 16 > buf.len() {
                break;
            }
            let hash = i32::from_le_bytes(buf[off..off+4].try_into().unwrap_or([0; 4]));
            let key = i32::from_le_bytes(buf[off+8..off+12].try_into().unwrap_or([0; 4]));
            let value = i32::from_le_bytes(buf[off+12..off+16].try_into().unwrap_or([0; 4]));

            let valid = hash >= 0
                && hash == key
                && key >= MIN_CARD_ID
                && key <= MAX_CARD_ID
                && value >= MIN_QUANTITY
                && value <= MAX_QUANTITY;

            if valid {
                if run_len == 0 {
                    run_start = start + off;
                }
                run_len += 1;
            } else {
                if run_len > best_run_len {
                    best_run_start = run_start;
                    best_run_len = run_len;
                }
                run_len = 0;
            }
        }
        if run_len > best_run_len {
            best_run_start = run_start;
            best_run_len = run_len;
        }
    }

    if debug {
        eprintln!(
            "mono::scan_heap_for_cards_dictionary: best run of {} consecutive valid entries at 0x{:x}",
            best_run_len, best_run_start,
        );
    }

    if best_run_len >= MIN_CONSECUTIVE {
        // Walk backwards from the run start to find the Entry[] object
        // header (vtable + sync + bounds + length at -0x20 from first
        // element). The run might not start at the first entry (some
        // initial entries may be empty/hash=-1), so look for the array
        // length at (run_start - 0x20 - N*16) + 0x18 for small N.
        //
        // For simplicity, return the run_start as-is. The caller
        // (read_cards_dictionary_entries) will iterate entries starting
        // from this address.
        best_run_start
    } else {
        0
    }
}

/// Read card entries starting from the first valid entry address
/// found by the scanner. Reads forward and backward from that
/// position until entries stop matching the card pattern.
pub fn read_cards_dictionary_entries(reader: &MemReader, first_entry_addr: usize) -> Vec<(i32, i32)> {
    const MIN_CARD_ID: i32 = 5_000;
    const MAX_CARD_ID: i32 = 200_000;
    const MIN_QUANTITY: i32 = 1;
    const MAX_QUANTITY: i32 = 4;
    const ENTRY_STRIDE: usize = 16;
    const MAX_ENTRIES: usize = 50_000;
    const MAX_GAP: usize = 200; // allow empty slots (hash=-1)

    let is_valid_entry = |addr: usize| -> Option<(i32, i32)> {
        let hash = reader.read_i32(addr);
        if hash == -1 { return None; } // empty slot, not an error
        let key = reader.read_i32(addr + 8);
        let value = reader.read_i32(addr + 12);
        if hash == key && key >= MIN_CARD_ID && key <= MAX_CARD_ID
            && value >= MIN_QUANTITY && value <= MAX_QUANTITY
        {
            Some((key, value))
        } else {
            None
        }
    };

    // Read backwards to find entries before the run start
    let mut backwards: Vec<(i32, i32)> = Vec::new();
    let mut gap = 0usize;
    for i in 1..MAX_ENTRIES {
        if let Some(addr) = first_entry_addr.checked_sub(i * ENTRY_STRIDE) {
            if addr < MIN_PTR { break; }
            let hash = reader.read_i32(addr);
            if hash == -1 { gap += 1; if gap > MAX_GAP { break; } continue; }
            if let Some(pair) = is_valid_entry(addr) {
                gap = 0;
                backwards.push(pair);
            } else {
                break; // hit non-entry data
            }
        } else {
            break;
        }
    }
    backwards.reverse();

    // Read forward
    let mut forward: Vec<(i32, i32)> = Vec::new();
    gap = 0;
    for i in 0..MAX_ENTRIES {
        let addr = first_entry_addr + i * ENTRY_STRIDE;
        let hash = reader.read_i32(addr);
        if hash == -1 { gap += 1; if gap > MAX_GAP { break; } continue; }
        if let Some(pair) = is_valid_entry(addr) {
            gap = 0;
            forward.push(pair);
        } else {
            break;
        }
    }

    let mut entries = backwards;
    entries.extend(forward);
    entries
}

/// Public entry point: read the card collection from a Mono-based Arena process.
pub fn read_mtga_cards_mono(process_name: &str) -> Result<Vec<(i32, i32)>, String> {
    let pid = find_wine_pid(process_name)?;
    let reader = MemReader::new(pid)?;

    let dict_addr = scan_heap_for_cards_dictionary(&reader, pid);
    if dict_addr == 0 {
        return Err(
            "Cards dictionary not found via Mono heap scan. \
             Either Arena isn't fully loaded or the Dictionary<int,int> \
             layout has changed."
                .to_string(),
        );
    }
    let entries = read_cards_dictionary_entries(&reader, dict_addr);
    if entries.is_empty() {
        return Err(format!(
            "Found Cards dictionary at 0x{:x} but it had no valid entries.",
            dict_addr,
        ));
    }
    Ok(entries)
}


// ──────────────────────────────────────────────────────────────────
// readMtgaInventory — scan for ClientPlayerInventory
// ──────────────────────────────────────────────────────────────────

/// Field offsets for ClientPlayerInventory (C# level, same across IL2CPP and Mono).
#[derive(Debug, Clone)]
pub struct InventoryFieldOffsets {
    pub wc_common: usize,
    pub wc_uncommon: usize,
    pub wc_rare: usize,
    pub wc_mythic: usize,
    pub gold: usize,
    pub gems: usize,
    pub vault_progress: usize,
}

pub fn resolve_inventory_field_offsets(
    fields: &[MonoFieldInfo],
) -> Option<InventoryFieldOffsets> {
    let find = |candidates: &[&str]| -> Option<usize> {
        for name in candidates {
            if let Some(f) = fields.iter().find(|f| !f.is_static && f.name == *name) {
                return Some(f.offset as usize);
            }
        }
        None
    };
    Some(InventoryFieldOffsets {
        wc_common: find(&["wcCommon", "<wcCommon>k__BackingField"])?,
        wc_uncommon: find(&["wcUncommon", "<wcUncommon>k__BackingField"])?,
        wc_rare: find(&["wcRare", "<wcRare>k__BackingField"])?,
        wc_mythic: find(&["wcMythic", "<wcMythic>k__BackingField"])?,
        gold: find(&["gold", "<gold>k__BackingField"])?,
        gems: find(&["gems", "<gems>k__BackingField"])?,
        vault_progress: find(&["vaultProgress", "<vaultProgress>k__BackingField"])?,
    })
}

fn inventory_plausible(wc: i32, wu: i32, wr: i32, wm: i32, g: i32, ge: i32) -> bool {
    // Very tight ranges + multi-field structural constraints.
    //
    // Ranges: wildcards ≤ 500, gold ≤ 50K, gems ≤ 50K.
    //
    // Structural (ALL required):
    //  - gold > 0 AND gems > 0 (every player past tutorial has both)
    //  - At least one wildcard > 0
    //  - common ≥ mythic (Arena gives ~10x more common WC than mythic)
    //
    // These combined constraints kill the false positives that
    // plague the score-only approach: random heap data almost never
    // has all six fields in tight ranges AND both currencies nonzero
    // AND the ordering constraint satisfied simultaneously.
    (0..=500).contains(&wc)
        && (0..=500).contains(&wu)
        && (0..=500).contains(&wr)
        && (0..=500).contains(&wm)
        && (1..=50_000).contains(&g)  // gold > 0 required
        && (1..=10_000).contains(&ge) // gems > 0 required (most players < 5K)
        && (wc + wu + wr + wm) > 0    // at least one wildcard
        && wc >= wm                    // structural ordering
}

fn inventory_score(wc: i32, wu: i32, wr: i32, wm: i32, g: i32, ge: i32) -> i64 {
    wc as i64 + wu as i64 + wr as i64 + wm as i64 + g as i64 + ge as i64
}

/// Scan heap for a ClientPlayerInventory instance.
/// Uses lazy class-name resolution: for each candidate that passes
/// the plausibility filter, resolve obj → vtable → class → name,
/// caching per unique class pointer.
/// Gold+gems anchored inventory search.
///
/// Instead of scanning every 8-byte position for an "object start"
/// and checking fields at fixed offsets (which produces too many
/// false positives from random heap data), this scanner:
///
/// 1. Scans for **adjacent i32 pairs (gold, gems)** in the heap.
///    Real currency values: both in [1, 50000], both nonzero.
/// 2. For each hit, reads backwards to check if wildcards exist at
///    the expected relative offsets (wcCommon at gold - 0x10, etc.).
/// 3. Checks vault progress (f64 in [0, 100]) at gold + 0x10.
/// 4. Scores by sum of all fields. Best candidate wins.
///
/// This is much more selective than the object-start scan because
/// the (gold, gems) pair is a strong 8-byte anchor, and the
/// relative wildcard positions provide structural verification.
pub fn scan_heap_for_client_player_inventory(
    reader: &MemReader,
    pid: u32,
    offsets: &InventoryFieldOffsets,
) -> Option<usize> {
    let debug = std::env::var("MTGA_DEBUG_MONO").is_ok();

    let heap_regions = find_scannable_heap_regions(pid);
    if debug {
        eprintln!(
            "mono::scan_heap_for_client_player_inventory: scanning {} regions (gold+gems anchored)",
            heap_regions.len(),
        );
    }

    // Relative offsets from gold to other fields (Mono layout)
    let wc_common_rel: isize = offsets.wc_common as isize - offsets.gold as isize;  // -0x10
    let wc_uncommon_rel: isize = offsets.wc_uncommon as isize - offsets.gold as isize; // -0x0C
    let wc_rare_rel: isize = offsets.wc_rare as isize - offsets.gold as isize;       // -0x08
    let wc_mythic_rel: isize = offsets.wc_mythic as isize - offsets.gold as isize;   // -0x04
    let gems_rel: usize = offsets.gems - offsets.gold;                               // 0x04
    let vault_rel: usize = offsets.vault_progress - offsets.gold;                    // 0x10

    // Need to read from gold-0x10 through gold+0x18 (vault is 8 bytes)
    let look_back = (-wc_common_rel) as usize; // 0x10
    let look_fwd = vault_rel + 8;              // 0x18

    // (gold_addr, score, [wc, wu, wr, wm, g, ge])
    let mut candidates: Vec<(usize, i64, [i32; 6])> = Vec::new();

    for (start, end) in heap_regions {
        let size = end - start;
        let buf = reader.read_bytes(start, size);
        if buf.len() != size {
            continue;
        }
        // Scan for adjacent (gold, gems) pairs at 4-byte alignment
        let mut i = look_back;
        while i + look_fwd <= buf.len() {
            let g = i32::from_le_bytes(buf[i..i+4].try_into().unwrap_or([0;4]));
            let ge = i32::from_le_bytes(buf[i+gems_rel..i+gems_rel+4].try_into().unwrap_or([0;4]));

            // Quick filter: both currencies must be positive and reasonable
            if !(1..=50_000).contains(&g) || !(1..=50_000).contains(&ge) {
                i += 4;
                continue;
            }

            // Read wildcards at relative offsets (backwards from gold)
            let wc_off = (i as isize + wc_common_rel) as usize;
            let wu_off = (i as isize + wc_uncommon_rel) as usize;
            let wr_off = (i as isize + wc_rare_rel) as usize;
            let wm_off = (i as isize + wc_mythic_rel) as usize;

            let wc = i32::from_le_bytes(buf[wc_off..wc_off+4].try_into().unwrap_or([0;4]));
            let wu = i32::from_le_bytes(buf[wu_off..wu_off+4].try_into().unwrap_or([0;4]));
            let wr = i32::from_le_bytes(buf[wr_off..wr_off+4].try_into().unwrap_or([0;4]));
            let wm = i32::from_le_bytes(buf[wm_off..wm_off+4].try_into().unwrap_or([0;4]));

            if !inventory_plausible(wc, wu, wr, wm, g, ge) {
                i += 4;
                continue;
            }

            // Check vault progress: f64 at gold + vault_rel should be in [0, 100]
            let vault_off = i + vault_rel;
            if vault_off + 8 > buf.len() {
                i += 4;
                continue;
            }
            let vault = f64::from_le_bytes(buf[vault_off..vault_off+8].try_into().unwrap_or([0;8]));
            // Vault progress constraints (combined eliminate nearly all
            // false positives):
            // 1. Range [5.0, 100.0] — excludes common false-positive
            //    floats like 1.0, 2.0, 3.0. Active players who've
            //    accumulated enough cards for meaningful inventory
            //    reading always have vault > 5%.
            // 2. "Round" to 1 decimal place — vault*10 must be very
            //    close to an integer (error < 1e-10). Real vault
            //    percentages from C# have IEEE 754 error ~1e-13.
            //    Random bytes rarely have this property.
            //
            // Players with vault < 5% can use the anchor mode
            // (pass known gold+gems values) instead.
            let vault_round = (vault * 10.0 - (vault * 10.0).round()).abs() < 1e-10;
            if !(vault >= 5.0 && vault <= 100.0 && vault_round) {
                i += 4;
                continue;
            }

            let score = inventory_score(wc, wu, wr, wm, g, ge);
            let gold_addr = start + i;
            if debug && candidates.len() < 20 {
                eprintln!(
                    "  hit gold_addr=0x{:x} wc=[{},{},{},{}] g={} ge={} vault={:.1} score={}",
                    gold_addr, wc, wu, wr, wm, g, ge, vault, score,
                );
            }
            candidates.push((gold_addr, score, [wc, wu, wr, wm, g, ge]));
            i += 4;
        }
    }

    candidates.sort_by_key(|(_, s, _)| std::cmp::Reverse(*s));
    if debug {
        eprintln!(
            "mono::scan_heap_for_client_player_inventory: {} candidates matched",
            candidates.len(),
        );
        for (addr, score, vals) in candidates.iter().take(10) {
            eprintln!(
                "  gold_addr=0x{:x} score={} wc=[{},{},{},{}] g={} ge={}",
                addr, score, vals[0], vals[1], vals[2], vals[3], vals[4], vals[5],
            );
        }
    }
    // Return the OBJECT start address, not the gold field address.
    // Object start = gold_addr - offsets.gold
    candidates.first().map(|(gold_addr, _, _)| gold_addr - offsets.gold)
}

/// Public entry point: read inventory from Mono Arena.
///
/// `known_gold`: if provided (> 0), uses the gold value as an exact
/// anchor to find the inventory object via byte-pattern search.
/// This is the most reliable mode — the caller reads their gold from
/// Arena's UI and passes it. Without it, falls back to the generic
/// scanner which may return false positives.
pub fn read_mtga_inventory_mono(
    process_name: &str,
    known_gold: i32,
    known_gems: i32,
) -> Result<(i32, i32, i32, i32, i32, i32, f64), String> {
    let pid = find_wine_pid(process_name)?;
    let reader = MemReader::new(pid)?;
    let debug = std::env::var("MTGA_DEBUG_MONO").is_ok();

    let offsets = InventoryFieldOffsets {
        wc_common: INV_WC_COMMON,
        wc_uncommon: INV_WC_UNCOMMON,
        wc_rare: INV_WC_RARE,
        wc_mythic: INV_WC_MYTHIC,
        gold: INV_GOLD,
        gems: INV_GEMS,
        vault_progress: INV_VAULT,
    };

    let inst = if known_gold > 0 && known_gems > 0 {
        // Exact anchor mode: find the gold+gems pair in the heap
        if debug {
            eprintln!("mono::read_mtga_inventory: using exact anchor gold={} gems={}", known_gold, known_gems);
        }
        let result = probe_heap_for_i32_pair(process_name, known_gold, known_gems)?;
        // The probe found gold at some address. Object start = gold_addr - INV_GOLD.
        // Parse the first HIT address from the probe result.
        let hit_line = result.lines().find(|l| l.starts_with("HIT at 0x"));
        let gold_addr = hit_line.and_then(|l| {
            let hex = l.strip_prefix("HIT at 0x")?.split(':').next()?;
            usize::from_str_radix(hex, 16).ok()
        }).ok_or_else(|| format!(
            "Could not find gold={} + gems={} adjacent in heap. Values may have changed.",
            known_gold, known_gems,
        ))?;
        gold_addr - offsets.gold
    } else {
        // Generic scanner mode (may hit false positives)
        scan_heap_for_client_player_inventory(&reader, pid, &offsets).ok_or_else(|| {
            "ClientPlayerInventory not found in Mono heap. Try passing known gold+gems values.".to_string()
        })?
    };

    let wc_common = reader.read_i32(inst + offsets.wc_common);
    let wc_uncommon = reader.read_i32(inst + offsets.wc_uncommon);
    let wc_rare = reader.read_i32(inst + offsets.wc_rare);
    let wc_mythic = reader.read_i32(inst + offsets.wc_mythic);
    let gold = reader.read_i32(inst + offsets.gold);
    let gems = reader.read_i32(inst + offsets.gems);
    let vault_progress = reader.read_f64(inst + offsets.vault_progress);

    if debug {
        eprintln!(
            "mono::read_mtga_inventory: inst=0x{:x} wc={{C:{}, U:{}, R:{}, M:{}}} gold={} gems={} vault={}",
            inst, wc_common, wc_uncommon, wc_rare, wc_mythic, gold, gems, vault_progress,
        );
        // Hex dump to discover actual field layout on Mono
        let bytes = reader.read_bytes(inst, 256);
        eprintln!("  raw hex dump of inventory object at 0x{:x}:", inst);
        for (ci, chunk) in bytes.chunks(16).enumerate() {
            // Print as hex AND as i32 values (4 bytes each)
            let hex: Vec<String> = chunk.iter().map(|b| format!("{:02x}", b)).collect();
            let mut i32s = Vec::new();
            for j in (0..chunk.len()).step_by(4) {
                if j + 4 <= chunk.len() {
                    let v = i32::from_le_bytes([chunk[j], chunk[j+1], chunk[j+2], chunk[j+3]]);
                    i32s.push(format!("{}", v));
                }
            }
            eprintln!("    +{:03x}: {}  i32s=[{}]", ci * 16, hex.join(" "), i32s.join(", "));
        }
    }
    Ok((wc_common, wc_uncommon, wc_rare, wc_mythic, gold, gems, vault_progress))
}

/// Probe: dump a MonoClass struct and try to find the name field.
/// Reads 256 bytes from the given class address, treats every 8-byte
/// offset as a potential pointer, dereferences it, and checks if it
/// gives a valid ASCII identifier string. Returns all matches.
pub fn probe_mono_class_name_offset(
    process_name: &str,
    class_addr: usize,
) -> Result<String, String> {
    let pid = find_wine_pid(process_name)?;
    let reader = MemReader::new(pid)?;
    let class_bytes = reader.read_bytes(class_addr, 256);
    let mut results = Vec::new();
    for off in (0..248).step_by(4) {
        // Try both 8-byte and 4-byte aligned pointers
        if off + 8 > class_bytes.len() { break; }
        let ptr = u64::from_le_bytes(
            class_bytes[off..off+8].try_into().unwrap_or([0;8]),
        ) as usize;
        if ptr < MIN_PTR || ptr > MAX_PTR { continue; }
        // Try reading as ASCII string
        let str_bytes = reader.read_bytes(ptr, 64);
        let mut s = String::new();
        for &b in &str_bytes {
            if b == 0 { break; }
            if b >= 32 && b < 127 {
                s.push(b as char);
            } else {
                s.clear();
                break;
            }
        }
        if s.len() >= 3 && s.chars().next().map_or(false, |c| c.is_ascii_alphabetic() || c == '_' || c == '<') {
            results.push(format!("+0x{:02x}: ptr=0x{:x} -> {:?}", off, ptr, s));
        }
    }
    Ok(results.join("\n"))
}

/// Read raw bytes at an arbitrary address in the target process.
pub fn read_bytes_at(process_name: &str, addr: usize, len: usize) -> Result<String, String> {
    let pid = find_wine_pid(process_name)?;
    let reader = MemReader::new(pid)?;
    let bytes = reader.read_bytes(addr, len);
    let hex_lines: Vec<String> = bytes.chunks(16).enumerate().map(|(i, chunk)| {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{:02x}", b)).collect();
        let mut i32s = Vec::new();
        for j in (0..chunk.len()).step_by(4) {
            if j + 4 <= chunk.len() {
                let v = i32::from_le_bytes([chunk[j], chunk[j+1], chunk[j+2], chunk[j+3]]);
                i32s.push(format!("{}", v));
            }
        }
        let mut ptrs = Vec::new();
        for j in (0..chunk.len()).step_by(8) {
            if j + 8 <= chunk.len() {
                let p = u64::from_le_bytes(chunk[j..j+8].try_into().unwrap());
                ptrs.push(format!("0x{:x}", p));
            }
        }
        format!("+{:03x}: {}  i32=[{}]  ptr=[{}]", i * 16, hex.join(" "), i32s.join(","), ptrs.join(","))
    }).collect();
    Ok(hex_lines.join("\n"))
}

// ──────────────────────────────────────────────────────────────────
// Offset discovery probe
// ──────────────────────────────────────────────────────────────────

/// Search the heap for a pair of adjacent i32 values (e.g. gold=1825,
/// gems=610). For each hit, dump 128 bytes before and after, plus
/// resolve the class name if the address looks like an object start.
/// Returns a JSON-formatted string with results.
pub fn probe_heap_for_i32_pair(
    process_name: &str,
    val_a: i32,
    val_b: i32,
) -> Result<String, String> {
    let pid = find_wine_pid(process_name)?;
    let reader = MemReader::new(pid)?;
    let heap_regions = find_scannable_heap_regions(pid);
    eprintln!(
        "probe_heap_for_i32_pair: searching {} regions for ({}, {}) adjacent",
        heap_regions.len(), val_a, val_b,
    );

    let target_a = val_a.to_le_bytes();
    let target_b = val_b.to_le_bytes();
    let mut hits: Vec<String> = Vec::new();

    for (start, end) in &heap_regions {
        let size = end - start;
        let buf = reader.read_bytes(*start, size);
        if buf.len() != size {
            continue;
        }
        // Search for val_a at offset i, val_b at offset i+4
        for i in 0..buf.len().saturating_sub(8) {
            if buf[i..i+4] == target_a && buf[i+4..i+8] == target_b {
                let abs_addr = start + i;
                eprintln!("  HIT at 0x{:x} (region 0x{:x}+0x{:x})", abs_addr, start, i);

                // Dump 128 bytes before the hit (find the object header)
                let dump_start = if i >= 128 { i - 128 } else { 0 };
                let dump_end = (i + 128).min(buf.len());
                let dump_region_start = start + dump_start;

                let mut lines = Vec::new();
                lines.push(format!("HIT at 0x{:x}:", abs_addr));
                for ci in (dump_start..dump_end).step_by(16) {
                    let row_end = (ci + 16).min(dump_end);
                    let hex: Vec<String> = buf[ci..row_end].iter().map(|b| format!("{:02x}", b)).collect();
                    let mut i32s = Vec::new();
                    for j in (ci..row_end).step_by(4) {
                        if j + 4 <= row_end {
                            let v = i32::from_le_bytes([buf[j], buf[j+1], buf[j+2], buf[j+3]]);
                            i32s.push(format!("{}", v));
                        }
                    }
                    let marker = if ci <= i && i < ci + 16 { " <---" } else { "" };
                    lines.push(format!(
                        "  +{:03x}: {}  [{}]{}",
                        start + ci - dump_region_start + dump_start, hex.join(" "), i32s.join(", "), marker,
                    ));
                }

                // Try to find an object header (vtable ptr) by looking
                // backward from the hit for a pointer-range value at
                // an 8-byte-aligned address
                let hit_in_region = i;
                let mut obj_start: Option<usize> = None;
                let search_back = hit_in_region.min(256);
                for back in (0..search_back).step_by(8) {
                    let candidate = hit_in_region - back;
                    if candidate + 8 > buf.len() {
                        continue;
                    }
                    let ptr = u64::from_le_bytes(
                        buf[candidate..candidate+8].try_into().unwrap_or([0;8]),
                    ) as usize;
                    if ptr >= MIN_PTR && ptr <= MAX_PTR {
                        // Check if this ptr dereferences to a class name
                        let class = reader.read_ptr(ptr); // vtable → class
                        if class >= MIN_PTR && class <= MAX_PTR {
                            let name = read_mono_class_name(&reader, class);
                            if !name.is_empty() && name.len() < 100 {
                                obj_start = Some(candidate);
                                let obj_abs = start + candidate;
                                let field_offset = i - candidate;
                                lines.push(format!(
                                    "  Likely object at 0x{:x} (field offset +0x{:x}), class={:?}",
                                    obj_abs, field_offset, name,
                                ));
                                break;
                            }
                        }
                    }
                }
                hits.push(lines.join("\n"));
                if hits.len() >= 10 {
                    break;
                }
            }
        }
        if hits.len() >= 10 {
            break;
        }
    }
    eprintln!("probe_heap_for_i32_pair: {} hits total", hits.len());
    Ok(hits.join("\n---\n"))
}

// ──────────────────────────────────────────────────────────────────
// Utility
// ──────────────────────────────────────────────────────────────────

/// Find the Wine Arena process PID. Uses `pgrep -f` with the Wine
/// process path pattern. Falls back to sysinfo.
fn find_wine_pid(process_name: &str) -> Result<u32, String> {
    // Try pgrep first (works on macOS for Wine processes)
    if let Ok(output) = Command::new("pgrep").arg("-f").arg(process_name).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(pid) = stdout.trim().lines().next().and_then(|s| s.parse::<u32>().ok()) {
            return Ok(pid);
        }
    }

    // Fallback: sysinfo (0.30.x — methods are inherent on System, no trait imports needed)
    use sysinfo::System;
    let mut sys = System::new_all();
    sys.refresh_all();
    for (pid, process) in sys.processes() {
        let name = process.name();
        if name.contains(process_name) {
            return Ok(pid.as_u32());
        }
    }

    Err(format!("Process '{}' not found", process_name))
}
