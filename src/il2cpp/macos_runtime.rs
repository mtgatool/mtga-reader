//! macOS IL2CPP runtime access.
//!
//! This is the macOS counterpart of `mono_reader` + `type_definition`: it opens
//! the MTGA task port, locates `GameAssembly.dylib`, finds the IL2CPP type-info
//! table and then resolves classes/fields **by name, from metadata**.
//!
//! Design notes (why this looks different from the older ad-hoc IL2CPP code):
//!
//! * Nothing about the object graph is hardcoded. Field offsets, dictionary
//!   entry strides and array element sizes are all read out of IL2CPP metadata,
//!   so a game update that reorders fields doesn't break us.
//! * The `Il2CppClass` layout itself is *detected* at init (anchored on the
//!   `klass` self-pointer) instead of trusting a version table.
//! * Metadata is immutable once the game has loaded, so it's served from a
//!   page cache; live object reads always go to the process.

#![cfg(target_os = "macos")]

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use mach2::kern_return::KERN_SUCCESS;
use mach2::port::mach_port_t;
use mach2::traps::{mach_task_self, task_for_pid};
use mach2::vm::{mach_vm_read_overwrite, mach_vm_region};
use mach2::vm_region::{vm_region_basic_info_64, VM_REGION_BASIC_INFO_64};

/// Size of the `Il2CppObject` header (klass + monitor). IL2CPP field offsets
/// are measured from the start of the object, header included.
pub const OBJ_HEADER: usize = 0x10;

const PAGE: usize = 0x10000; // 64 KiB metadata cache granularity

// ---------------------------------------------------------------------------
// Il2CppType type codes (Il2CppTypeEnum)
// ---------------------------------------------------------------------------

pub mod type_enum {
    pub const VOID: u8 = 0x01;
    pub const BOOLEAN: u8 = 0x02;
    pub const CHAR: u8 = 0x03;
    pub const I1: u8 = 0x04;
    pub const U1: u8 = 0x05;
    pub const I2: u8 = 0x06;
    pub const U2: u8 = 0x07;
    pub const I4: u8 = 0x08;
    pub const U4: u8 = 0x09;
    pub const I8: u8 = 0x0a;
    pub const U8: u8 = 0x0b;
    pub const R4: u8 = 0x0c;
    pub const R8: u8 = 0x0d;
    pub const STRING: u8 = 0x0e;
    pub const PTR: u8 = 0x0f;
    pub const VALUETYPE: u8 = 0x11;
    pub const CLASS: u8 = 0x12;
    pub const VAR: u8 = 0x13;
    pub const ARRAY: u8 = 0x14;
    pub const GENERICINST: u8 = 0x15;
    pub const I: u8 = 0x18;
    pub const U: u8 = 0x19;
    pub const OBJECT: u8 = 0x1c;
    pub const SZARRAY: u8 = 0x1d;
    pub const MVAR: u8 = 0x1e;
}

/// FIELD_ATTRIBUTE_STATIC
const FIELD_ATTR_STATIC: u32 = 0x10;
/// FIELD_ATTRIBUTE_LITERAL (compile-time constant, no storage)
const FIELD_ATTR_LITERAL: u32 = 0x40;

// ---------------------------------------------------------------------------
// Il2CppClass layout
// ---------------------------------------------------------------------------

/// Byte offsets inside `Il2CppClass`. Defaults match Unity 2019+ (through
/// 2022.x); `detect` re-anchors them against the live process.
#[derive(Clone, Copy, Debug)]
pub struct ClassLayout {
    pub name: usize,
    pub namespace: usize,
    pub element_class: usize,
    pub cast_class: usize,
    pub declaring_type: usize,
    pub parent: usize,
    pub generic_class: usize,
    pub type_definition: usize,
    pub klass_self: usize,
    pub fields: usize,
    pub static_fields: usize,
    pub instance_size: usize,
    pub element_size: usize,
    pub flags: usize,
    pub field_info_size: usize,
}

impl Default for ClassLayout {
    fn default() -> Self {
        ClassLayout {
            name: 0x10,
            namespace: 0x18,
            element_class: 0x40,
            cast_class: 0x48,
            declaring_type: 0x50,
            parent: 0x58,
            generic_class: 0x60,
            type_definition: 0x68,
            klass_self: 0x78,
            fields: 0x80,
            static_fields: 0xB8,
            instance_size: 0xF8,
            // Verified against arrays of known element size on the live MTGA
            // build (String[]=8, Entry<uint,int>[]=16, Entry<Guid,_>[]=32).
            // Note 0xFC reads a constant 8 for every array class, so it is a
            // convincing-looking wrong answer — `array_element_size`
            // cross-checks the value against the element class.
            element_size: 0x104,
            flags: 0x114,
            field_info_size: 0x20,
        }
    }
}

impl ClassLayout {
    /// Re-derive the layout from sample class pointers by locating the `klass`
    /// self-pointer, which anchors the whole struct.
    ///
    /// Several members self-reference (`element_class` and `castClass` both
    /// point at the class itself for ordinary types), so the anchor is
    /// disambiguated by requiring that the pointer *right after* it is a
    /// `FieldInfo` array whose `parent` points back at the class.
    fn detect(mem: &Mem, samples: &[usize]) -> Option<ClassLayout> {
        let mut layout = ClassLayout::default();

        let mut best: Option<(usize, usize)> = None;
        for anchor in (0x20..0x100).step_by(8) {
            let mut score = 0usize;
            for &class in samples {
                if !mem.is_mapped(class + anchor + 8) || mem.meta_ptr(class + anchor) != class {
                    continue;
                }
                let fields = mem.meta_ptr(class + anchor + 8);
                if !plausible(fields) || !mem.is_mapped(fields + 0x18) {
                    continue;
                }
                // FieldInfo { name; type; parent; offset; token }
                if mem.meta_ptr(fields + 0x10) != class {
                    continue;
                }
                if mem
                    .meta_cstr(mem.meta_ptr(fields))
                    .map(|s| !s.is_empty())
                    .unwrap_or(false)
                {
                    score += 1;
                }
            }
            if score >= 3 && best.map(|b| score > b.1).unwrap_or(true) {
                best = Some((anchor, score));
            }
        }

        let anchor = best?.0;

        if anchor != layout.klass_self {
            let shift = anchor as isize - layout.klass_self as isize;
            let adj = |v: usize| (v as isize + shift) as usize;
            layout.element_class = adj(layout.element_class);
            layout.cast_class = adj(layout.cast_class);
            layout.declaring_type = adj(layout.declaring_type);
            layout.parent = adj(layout.parent);
            layout.generic_class = adj(layout.generic_class);
            layout.type_definition = adj(layout.type_definition);
            layout.klass_self = anchor;
            layout.fields = anchor + 8;
            layout.static_fields = adj(layout.static_fields);
            layout.instance_size = adj(layout.instance_size);
            layout.element_size = adj(layout.element_size);
            layout.flags = adj(layout.flags);
        }

        Some(layout)
    }
}

/// One field, as described by IL2CPP metadata.
#[derive(Clone, Debug)]
pub struct FieldRec {
    pub name: String,
    /// Offset from the start of the object (header included) for instance
    /// fields, or into the static storage block for statics.
    pub offset: i32,
    pub type_ptr: usize,
    /// `Il2CppTypeEnum` of the field's type.
    pub type_code: u8,
    pub is_static: bool,
    /// Thread-static fields have no fixed offset; we can't read them.
    pub is_thread_static: bool,
}

// ---------------------------------------------------------------------------
// Memory access
// ---------------------------------------------------------------------------

/// A mapped VM region in the target process.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub start: usize,
    pub end: usize,
    pub prot: i32,
}

/// Raw mach memory access to another process.
pub struct Mem {
    task: mach_port_t,
    pub pid: u32,
    /// Sorted, non-overlapping mapped regions, refreshed on demand.
    regions: RefCell<Vec<Region>>,
    /// Page cache for immutable metadata reads.
    cache: RefCell<HashMap<usize, Arc<Vec<u8>>>>,
}

impl Mem {
    pub fn open(pid: u32) -> Result<Mem, String> {
        let mut task: mach_port_t = 0;
        let kr = unsafe { task_for_pid(mach_task_self(), pid as i32, &mut task) };
        if kr != KERN_SUCCESS {
            return Err(format!(
                "task_for_pid({pid}) failed with {kr}. Run as root (sudo), or sign the host \
                 binary with com.apple.security.cs.debugger."
            ));
        }

        let mem = Mem {
            task,
            pid,
            regions: RefCell::new(Vec::new()),
            cache: RefCell::new(HashMap::new()),
        };
        mem.refresh_regions();
        Ok(mem)
    }

    /// Walk the target's VM map. Used both to find `GameAssembly.dylib` and to
    /// reject bogus pointers without paying for a failing syscall each time.
    pub fn refresh_regions(&self) {
        let mut out = Vec::new();
        let mut addr: u64 = 1;

        loop {
            let mut size: u64 = 0;
            let mut info = vm_region_basic_info_64::default();
            let mut count = (std::mem::size_of::<vm_region_basic_info_64>()
                / std::mem::size_of::<i32>()) as u32;
            let mut object_name: mach_port_t = 0;

            let kr = unsafe {
                mach_vm_region(
                    self.task,
                    &mut addr,
                    &mut size,
                    VM_REGION_BASIC_INFO_64,
                    &mut info as *mut _ as *mut i32,
                    &mut count,
                    &mut object_name,
                )
            };

            if kr != KERN_SUCCESS || size == 0 {
                break;
            }

            out.push(Region {
                start: addr as usize,
                end: (addr + size) as usize,
                prot: info.protection,
            });

            addr = addr.saturating_add(size);
            if addr == 0 || out.len() > 200_000 {
                break;
            }
        }

        out.sort_by_key(|r| r.start);
        *self.regions.borrow_mut() = out;
    }

    pub fn regions(&self) -> Vec<Region> {
        self.regions.borrow().clone()
    }

    /// The region containing `addr`, if any (binary search, no syscall).
    fn region_of(&self, addr: usize) -> Option<Region> {
        let regions = self.regions.borrow();
        let idx = match regions.binary_search_by_key(&addr, |r| r.start) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        let r = regions[idx];
        if addr >= r.start && addr < r.end {
            Some(r)
        } else {
            None
        }
    }

    /// Is `addr` readable? Cheap pre-filter for pointer validation.
    pub fn is_mapped(&self, addr: usize) -> bool {
        self.region_of(addr).map(|r| r.prot & 1 != 0).unwrap_or(false)
    }

    // -- uncached (live) reads ------------------------------------------------

    pub fn read_bytes(&self, addr: usize, len: usize) -> Vec<u8> {
        let mut buf = vec![0u8; len];
        if !self.read_into(addr, &mut buf) {
            return Vec::new();
        }
        buf
    }

    fn read_into(&self, addr: usize, buf: &mut [u8]) -> bool {
        if addr == 0 || buf.is_empty() {
            return false;
        }
        let mut out: u64 = 0;
        let kr = unsafe {
            mach_vm_read_overwrite(
                self.task,
                addr as u64,
                buf.len() as u64,
                buf.as_mut_ptr() as u64,
                &mut out,
            )
        };
        kr == KERN_SUCCESS && out as usize == buf.len()
    }

    pub fn read_ptr(&self, addr: usize) -> usize {
        let b = self.read_bytes(addr, 8);
        if b.len() < 8 {
            return 0;
        }
        usize::from_le_bytes(b[..8].try_into().unwrap())
    }

    pub fn read_u32(&self, addr: usize) -> u32 {
        let b = self.read_bytes(addr, 4);
        if b.len() < 4 {
            return 0;
        }
        u32::from_le_bytes(b[..4].try_into().unwrap())
    }

    pub fn read_i32(&self, addr: usize) -> i32 {
        self.read_u32(addr) as i32
    }

    pub fn read_u64(&self, addr: usize) -> u64 {
        self.read_ptr(addr) as u64
    }

    pub fn read_i64(&self, addr: usize) -> i64 {
        self.read_ptr(addr) as i64
    }

    pub fn read_u16(&self, addr: usize) -> u16 {
        let b = self.read_bytes(addr, 2);
        if b.len() < 2 {
            return 0;
        }
        u16::from_le_bytes([b[0], b[1]])
    }

    pub fn read_u8(&self, addr: usize) -> u8 {
        self.read_bytes(addr, 1).first().copied().unwrap_or(0)
    }

    pub fn read_f32(&self, addr: usize) -> f32 {
        f32::from_bits(self.read_u32(addr))
    }

    pub fn read_f64(&self, addr: usize) -> f64 {
        f64::from_bits(self.read_u64(addr))
    }

    // -- cached (metadata) reads ---------------------------------------------

    /// Fetch the 64 KiB page containing `addr`, clamped to its VM region so a
    /// page straddling an unmapped boundary still succeeds.
    fn page(&self, addr: usize) -> Option<Arc<Vec<u8>>> {
        let base = addr & !(PAGE - 1);
        if let Some(p) = self.cache.borrow().get(&base) {
            return Some(p.clone());
        }

        let region = self.region_of(addr)?;
        if region.prot & 1 == 0 {
            return None;
        }
        let start = base.max(region.start);
        let end = (base + PAGE).min(region.end);
        if end <= start {
            return None;
        }

        let mut buf = vec![0u8; end - start];
        if !self.read_into(start, &mut buf) {
            return None;
        }

        // Normalise to a full page so indexing is uniform.
        let mut page = vec![0u8; PAGE];
        page[(start - base)..(start - base + buf.len())].copy_from_slice(&buf);
        let page = Arc::new(page);
        self.cache.borrow_mut().insert(base, page.clone());
        Some(page)
    }

    /// Read immutable metadata bytes (served from the page cache).
    pub fn meta_bytes(&self, addr: usize, len: usize) -> Option<Vec<u8>> {
        if addr == 0 || len == 0 {
            return None;
        }
        let mut out = Vec::with_capacity(len);
        let mut cur = addr;
        while out.len() < len {
            let page = self.page(cur)?;
            let base = cur & !(PAGE - 1);
            let off = cur - base;
            let take = (PAGE - off).min(len - out.len());
            out.extend_from_slice(&page[off..off + take]);
            cur += take;
        }
        Some(out)
    }

    pub fn meta_ptr(&self, addr: usize) -> usize {
        self.meta_bytes(addr, 8)
            .map(|b| usize::from_le_bytes(b[..8].try_into().unwrap()))
            .unwrap_or(0)
    }

    pub fn meta_u32(&self, addr: usize) -> u32 {
        self.meta_bytes(addr, 4)
            .map(|b| u32::from_le_bytes(b[..4].try_into().unwrap()))
            .unwrap_or(0)
    }

    /// Read a NUL-terminated C string from metadata.
    pub fn meta_cstr(&self, addr: usize) -> Option<String> {
        if addr == 0 || !self.is_mapped(addr) {
            return None;
        }
        let bytes = self.meta_bytes(addr, 256)?;
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        if end == 0 {
            return Some(String::new());
        }
        let s = &bytes[..end];
        if !s.iter().all(|&b| (0x20..0x7f).contains(&b)) {
            return None;
        }
        Some(String::from_utf8_lossy(s).into_owned())
    }

    /// Address of the target's `dyld_all_image_infos`, via `TASK_DYLD_INFO`.
    pub fn dyld_all_image_infos(&self) -> Option<usize> {
        use mach2::task::task_info;
        use mach2::task_info::{task_dyld_info, TASK_DYLD_INFO};

        let mut info = task_dyld_info {
            all_image_info_addr: 0,
            all_image_info_size: 0,
            all_image_info_format: 0,
        };
        let mut count =
            (std::mem::size_of::<task_dyld_info>() / std::mem::size_of::<i32>()) as u32;

        let kr = unsafe {
            task_info(
                self.task,
                TASK_DYLD_INFO,
                &mut info as *mut _ as *mut i32,
                &mut count,
            )
        };

        if kr != KERN_SUCCESS || info.all_image_info_addr == 0 {
            return None;
        }
        Some(info.all_image_info_addr as usize)
    }

    /// Read a `System.String` (UTF-16, length at +0x10, chars at +0x14).
    pub fn read_managed_string(&self, addr: usize) -> Option<String> {
        if !plausible(addr) {
            return None;
        }
        let len = self.read_i32(addr + 0x10);
        if len < 0 || len > 100_000 {
            return None;
        }
        if len == 0 {
            return Some(String::new());
        }
        let bytes = self.read_bytes(addr + 0x14, len as usize * 2);
        if bytes.len() < len as usize * 2 {
            return None;
        }
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Some(String::from_utf16_lossy(&units))
    }
}

/// Can we obtain a task port for `pid` right now?
///
/// On macOS "do we have the privileges to read memory" is *not* the same
/// question as "are we root": a binary signed with
/// `com.apple.security.cs.debugger` can attach as a normal user. This probes
/// the real capability instead of inferring it from the uid.
pub fn can_attach(pid: u32) -> bool {
    unsafe {
        let mut task: mach_port_t = 0;
        let kr = task_for_pid(mach_task_self(), pid as i32, &mut task);
        if kr == KERN_SUCCESS {
            // Don't leak the send right — this is polled by status UIs.
            mach2::mach_port::mach_port_deallocate(mach_task_self(), task);
            true
        } else {
            false
        }
    }
}

/// Is this plausibly a heap/metadata pointer rather than garbage?
pub fn plausible(addr: usize) -> bool {
    addr > 0x10000 && addr < 0x0000_7FFF_FFFF_FFFF && addr % 8 == 0
}

// ---------------------------------------------------------------------------
// The runtime
// ---------------------------------------------------------------------------

pub struct Il2Cpp {
    pub mem: Mem,
    pub layout: ClassLayout,
    pub type_info_table: usize,
    pub type_count: usize,
    /// GameAssembly.dylib __DATA segment (base, size) the table pointer lives in.
    pub data_segment: (usize, usize),
    /// Offset of the table pointer within that segment (diagnostic).
    pub table_ptr_offset: usize,
    fields: RefCell<HashMap<usize, Arc<Vec<FieldRec>>>>,
    classes: RefCell<HashMap<String, usize>>,
    /// `Il2CppTypeDefinition*` -> `Il2CppClass*`, built lazily (see `type_class`).
    type_defs: RefCell<Option<HashMap<usize, usize>>>,
}

impl Il2Cpp {
    pub fn attach(pid: u32) -> Result<Il2Cpp, String> {
        let mem = Mem::open(pid)?;

        let segments = find_game_assembly_data(&mem)
            .ok_or_else(|| "could not locate the GameAssembly.dylib __DATA segments".to_string())?;

        // The globals live in one of the writable segments; take the best
        // candidate across all of them.
        let mut best: Option<(usize, usize, ClassLayout, usize, (usize, usize))> = None;
        for (base, size) in segments {
            if let Some((table, off, layout, count)) = find_type_info_table(&mem, base, size) {
                if best.as_ref().map(|b| count > b.3).unwrap_or(true) {
                    best = Some((table, off, layout, count, (base, size)));
                }
            }
        }

        let (table, table_off, layout, count, data_segment) = best.ok_or_else(|| {
            "could not locate the IL2CPP type-info table in GameAssembly's data segments"
                .to_string()
        })?;

        Ok(Il2Cpp {
            mem,
            layout,
            type_info_table: table,
            type_count: count,
            data_segment,
            table_ptr_offset: table_off,
            fields: RefCell::new(HashMap::new()),
            classes: RefCell::new(HashMap::new()),
            type_defs: RefCell::new(None),
        })
    }

    // -- class metadata -------------------------------------------------------

    pub fn class_name(&self, class: usize) -> String {
        if !plausible(class) {
            return String::new();
        }
        self.mem
            .meta_cstr(self.mem.meta_ptr(class + self.layout.name))
            .unwrap_or_default()
    }

    pub fn class_namespace(&self, class: usize) -> String {
        if !plausible(class) {
            return String::new();
        }
        self.mem
            .meta_cstr(self.mem.meta_ptr(class + self.layout.namespace))
            .unwrap_or_default()
    }

    pub fn class_parent(&self, class: usize) -> usize {
        if !plausible(class) {
            return 0;
        }
        let p = self.mem.meta_ptr(class + self.layout.parent);
        if self.is_class(p) {
            p
        } else {
            0
        }
    }

    pub fn class_of(&self, obj: usize) -> usize {
        if !plausible(obj) {
            return 0;
        }
        let c = self.mem.read_ptr(obj);
        if self.is_class(c) {
            c
        } else {
            0
        }
    }

    /// A pointer is an `Il2CppClass` iff its `klass` self-pointer points back.
    pub fn is_class(&self, addr: usize) -> bool {
        plausible(addr)
            && self.mem.is_mapped(addr + self.layout.klass_self)
            && self.mem.meta_ptr(addr + self.layout.klass_self) == addr
    }

    /// Resolve the `Il2CppClass` an `Il2CppType` refers to.
    ///
    /// The `data` union is rarely a class pointer. On this build it holds an
    /// `Il2CppMetadataTypeHandle` — a pointer to the `Il2CppTypeDefinition` in
    /// global-metadata — which is exactly what each class stores at
    /// `class + type_definition`. So the mapping is done by inverting that,
    /// built lazily since only the explorer/`readData` paths need type names.
    pub fn type_class(&self, type_ptr: usize) -> usize {
        if !plausible(type_ptr) {
            return 0;
        }
        let data = self.mem.meta_ptr(type_ptr);
        if data == 0 {
            return 0;
        }

        if self.is_class(data) {
            return data;
        }

        if let Some(&c) = self.type_def_map().get(&data) {
            return c;
        }

        // Il2CppGenericClass { type; context (2 ptrs); cached_class }
        if plausible(data) && self.mem.is_mapped(data + 0x18) {
            let c = self.mem.meta_ptr(data + 0x18);
            if self.is_class(c) {
                return c;
            }
        }

        0
    }

    /// `Il2CppTypeDefinition*` -> `Il2CppClass*`, built once from the type table.
    fn type_def_map(&self) -> std::cell::Ref<'_, HashMap<usize, usize>> {
        if self.type_defs.borrow().is_none() {
            let mut map = HashMap::with_capacity(self.type_count);
            if let Some(table) = self.mem.meta_bytes(self.type_info_table, self.type_count * 8) {
                for chunk in table.chunks_exact(8) {
                    let class = usize::from_le_bytes(chunk.try_into().unwrap());
                    if !plausible(class) || !self.mem.is_mapped(class + self.layout.type_definition)
                    {
                        continue;
                    }
                    let handle = self.mem.meta_ptr(class + self.layout.type_definition);
                    if handle != 0 {
                        map.entry(handle).or_insert(class);
                    }
                }
            }
            *self.type_defs.borrow_mut() = Some(map);
        }

        std::cell::Ref::map(self.type_defs.borrow(), |o| o.as_ref().unwrap())
    }

    /// Display name of a field's declared type (falls back to the type code).
    pub fn type_name(&self, type_ptr: usize, type_code: u8) -> String {
        let c = self.type_class(type_ptr);
        let n = self.class_name(c);
        if n.is_empty() {
            format!("code_0x{type_code:02x}")
        } else {
            n
        }
    }

    pub fn class_element_class(&self, class: usize) -> usize {
        self.mem.meta_ptr(class + self.layout.element_class)
    }

    pub fn class_element_size(&self, class: usize) -> u32 {
        self.mem.meta_u32(class + self.layout.element_size)
    }

    pub fn class_instance_size(&self, class: usize) -> u32 {
        self.mem.meta_u32(class + self.layout.instance_size)
    }

    pub fn class_static_storage(&self, class: usize) -> usize {
        self.mem.meta_ptr(class + self.layout.static_fields)
    }

    /// Fields declared directly on `class` (not inherited), from metadata.
    ///
    /// Terminates on `FieldInfo.parent != class`, which is self-validating and
    /// avoids depending on where `field_count` sits in `Il2CppClass`.
    pub fn class_fields(&self, class: usize) -> Arc<Vec<FieldRec>> {
        if let Some(f) = self.fields.borrow().get(&class) {
            return f.clone();
        }

        let mut out = Vec::new();
        let base = self.mem.meta_ptr(class + self.layout.fields);

        if plausible(base) && self.is_class(class) {
            for i in 0..4096usize {
                let fi = base + i * self.layout.field_info_size;
                if !self.mem.is_mapped(fi) {
                    break;
                }
                if self.mem.meta_ptr(fi + 0x10) != class {
                    break; // end of this class's field array
                }
                let name = match self.mem.meta_cstr(self.mem.meta_ptr(fi)) {
                    Some(n) if !n.is_empty() => n,
                    _ => break,
                };
                let type_ptr = self.mem.meta_ptr(fi + 0x08);
                let bits = self.mem.meta_u32(type_ptr + 0x08);
                let attrs = bits & 0xFFFF;
                let type_code = ((bits >> 16) & 0xFF) as u8;
                let offset = self.mem.meta_u32(fi + 0x18) as i32;

                if attrs & FIELD_ATTR_LITERAL != 0 {
                    continue; // const: no storage
                }

                out.push(FieldRec {
                    name,
                    offset,
                    type_ptr,
                    type_code,
                    is_static: attrs & FIELD_ATTR_STATIC != 0,
                    is_thread_static: offset == -1,
                });
            }
        }

        let out = Arc::new(out);
        self.fields.borrow_mut().insert(class, out.clone());
        out
    }

    /// Find a field by name on `class`, walking up the inheritance chain.
    pub fn find_field(&self, class: usize, name: &str) -> Option<FieldRec> {
        let mut cur = class;
        for _ in 0..32 {
            if !self.is_class(cur) {
                break;
            }
            if let Some(f) = self.class_fields(cur).iter().find(|f| f.name == name) {
                return Some(f.clone());
            }
            cur = self.class_parent(cur);
            if cur == 0 {
                break;
            }
        }
        None
    }

    /// Look up a class by name in the type-info table (result is cached).
    pub fn find_class(&self, name: &str) -> Option<usize> {
        if let Some(&c) = self.classes.borrow().get(name) {
            return Some(c);
        }

        let table = self.mem.meta_bytes(self.type_info_table, self.type_count * 8)?;
        for chunk in table.chunks_exact(8) {
            let class = usize::from_le_bytes(chunk.try_into().unwrap());
            if !plausible(class) || !self.mem.is_mapped(class + self.layout.name) {
                continue;
            }
            let n = self
                .mem
                .meta_cstr(self.mem.meta_ptr(class + self.layout.name));
            if n.as_deref() == Some(name) && self.is_class(class) {
                self.classes.borrow_mut().insert(name.to_string(), class);
                return Some(class);
            }
        }
        None
    }

    // -- instance access ------------------------------------------------------

    /// Address of a field's storage on `obj` (resolved by name via the object's
    /// actual runtime class, so inherited fields work).
    pub fn field_addr(&self, obj: usize, name: &str) -> Option<(usize, FieldRec)> {
        let class = self.class_of(obj);
        if class == 0 {
            return None;
        }
        let f = self.find_field(class, name)?;
        if f.is_thread_static {
            return None;
        }
        if f.is_static {
            let storage = self.class_static_storage(class);
            if !plausible(storage) {
                return None;
            }
            return Some((storage + f.offset as usize, f));
        }
        Some((obj + f.offset as usize, f))
    }

    /// Read a static field's storage address on a *class*.
    pub fn static_field_addr(&self, class: usize, name: &str) -> Option<(usize, FieldRec)> {
        let f = self.find_field(class, name)?;
        if !f.is_static || f.is_thread_static {
            return None;
        }
        let storage = self.class_static_storage(class);
        if !plausible(storage) {
            return None;
        }
        Some((storage + f.offset as usize, f))
    }

    /// Follow a reference field on `obj`.
    pub fn ref_field(&self, obj: usize, name: &str) -> Option<usize> {
        let (addr, _) = self.field_addr(obj, name)?;
        let child = self.mem.read_ptr(addr);
        if plausible(child) {
            Some(child)
        } else {
            None
        }
    }

    pub fn string_field(&self, obj: usize, name: &str) -> Option<String> {
        let (addr, _) = self.field_addr(obj, name)?;
        self.mem.read_managed_string(self.mem.read_ptr(addr))
    }

    pub fn i32_field(&self, obj: usize, name: &str) -> Option<i32> {
        self.field_addr(obj, name).map(|(a, _)| self.mem.read_i32(a))
    }

    pub fn u32_field(&self, obj: usize, name: &str) -> Option<u32> {
        self.field_addr(obj, name).map(|(a, _)| self.mem.read_u32(a))
    }

    pub fn f64_field(&self, obj: usize, name: &str) -> Option<f64> {
        self.field_addr(obj, name).map(|(a, _)| self.mem.read_f64(a))
    }

    /// Read a numeric field, honouring the metadata type code. Used for fields
    /// whose C# type we don't want to assume (e.g. `vaultProgress`, which is a
    /// double on Windows but worth reading generically).
    pub fn number_field(&self, obj: usize, name: &str) -> Option<serde_json::Value> {
        let (addr, f) = self.field_addr(obj, name)?;
        Some(self.read_number(addr, f.type_code))
    }

    /// Decode a number straight out of an already-fetched buffer. Lets bulk
    /// container reads avoid a syscall per element.
    pub fn decode_number(buf: &[u8], code: u8) -> serde_json::Value {
        use type_enum::*;
        let u32v = || {
            u32::from_le_bytes(buf.get(..4).and_then(|b| b.try_into().ok()).unwrap_or([0; 4]))
        };
        let u64v = || {
            u64::from_le_bytes(buf.get(..8).and_then(|b| b.try_into().ok()).unwrap_or([0; 8]))
        };
        match code {
            BOOLEAN => serde_json::json!(buf.first().copied().unwrap_or(0) != 0),
            I1 => serde_json::json!(buf.first().copied().unwrap_or(0) as i8),
            U1 => serde_json::json!(buf.first().copied().unwrap_or(0)),
            I2 => serde_json::json!(u32v() as u16 as i16),
            U2 | CHAR => serde_json::json!(u32v() as u16),
            I4 | VALUETYPE => serde_json::json!(u32v() as i32),
            U4 => serde_json::json!(u32v()),
            I8 | I => serde_json::json!(u64v() as i64),
            U8 | U => serde_json::json!(u64v()),
            R4 => serde_json::json!(f32::from_bits(u32v())),
            R8 => serde_json::json!(f64::from_bits(u64v())),
            _ => serde_json::json!(u32v() as i32),
        }
    }

    pub fn read_number(&self, addr: usize, code: u8) -> serde_json::Value {
        use type_enum::*;
        match code {
            BOOLEAN => serde_json::json!(self.mem.read_u8(addr) != 0),
            I1 => serde_json::json!(self.mem.read_u8(addr) as i8),
            U1 => serde_json::json!(self.mem.read_u8(addr)),
            I2 => serde_json::json!(self.mem.read_u16(addr) as i16),
            U2 | CHAR => serde_json::json!(self.mem.read_u16(addr)),
            I4 => serde_json::json!(self.mem.read_i32(addr)),
            U4 => serde_json::json!(self.mem.read_u32(addr)),
            I8 | I => serde_json::json!(self.mem.read_i64(addr)),
            U8 | U => serde_json::json!(self.mem.read_u64(addr)),
            R4 => serde_json::json!(self.mem.read_f32(addr)),
            R8 => serde_json::json!(self.mem.read_f64(addr)),
            _ => serde_json::json!(self.mem.read_i32(addr)),
        }
    }

    /// Read a `System.Guid` value type (16 bytes) in canonical form.
    pub fn read_guid(&self, addr: usize) -> String {
        let b = self.mem.read_bytes(addr, 16);
        if b.len() < 16 {
            return String::new();
        }
        format!(
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            u16::from_le_bytes([b[4], b[5]]),
            u16::from_le_bytes([b[6], b[7]]),
            b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
        )
    }

    // -- containers -----------------------------------------------------------

    /// Stride of an array's elements.
    ///
    /// The runtime guarantees this is either a pointer (reference elements) or
    /// the element type's inline size (value-type elements), so a candidate
    /// word is only accepted when it matches one of those two. That makes a
    /// wrong `element_size` offset fail loudly instead of silently.
    pub fn array_element_size(&self, arr_class: usize, elem_class: usize) -> usize {
        let inline = (self.class_instance_size(elem_class) as usize).saturating_sub(OBJ_HEADER);

        for off in [self.layout.element_size, 0x100, 0xFC, 0x108, 0x110] {
            let v = self.mem.meta_u32(arr_class + off) as usize;
            if v == 8 || (inline > 0 && v == inline) {
                return v;
            }
        }

        // Nothing matched: fall back to the element class's own size.
        if inline > 0 && inline <= 0x10000 {
            inline
        } else {
            8
        }
    }

    /// `(element_class, element_size, length, data_addr)` for an `Il2CppArray`.
    pub fn array_info(&self, array: usize) -> Option<(usize, usize, i32, usize)> {
        if !plausible(array) {
            return None;
        }
        let class = self.class_of(array);
        if class == 0 {
            return None;
        }
        let elem_class = self.class_element_class(class);
        let elem_size = self.array_element_size(class, elem_class);
        let len = self.mem.read_i32(array + 0x18);
        if elem_size == 0 || elem_size > 0x10000 || len < 0 {
            return None;
        }
        Some((elem_class, elem_size, len, array + 0x20))
    }

    /// Resolve `Entry` field offsets for a `Dictionary<K,V>`'s entries array.
    ///
    /// `Entry` is a value type, so metadata offsets include the object header;
    /// unboxed array elements need it subtracted.
    fn entry_layout(&self, entry_class: usize) -> Option<EntryLayout> {
        let fields = self.class_fields(entry_class);
        let get = |n: &str| {
            fields
                .iter()
                .find(|f| f.name == n)
                .map(|f| (f.offset as usize).saturating_sub(OBJ_HEADER))
        };
        Some(EntryLayout {
            hash: get("hashCode")?,
            key: get("key")?,
            value: get("value")?,
            key_code: fields.iter().find(|f| f.name == "key").map(|f| f.type_code)?,
            value_code: fields
                .iter()
                .find(|f| f.name == "value")
                .map(|f| f.type_code)?,
        })
    }

    /// Iterate a `Dictionary<K,V>`'s live entries, yielding
    /// `(key_addr, key_code, value_addr, value_code)`.
    pub fn dict_entries(&self, dict: usize, max: usize) -> Vec<(usize, u8, usize, u8)> {
        let mut out = Vec::new();
        let entries = match self.ref_field(dict, "_entries").or_else(|| self.ref_field(dict, "entries")) {
            Some(e) => e,
            None => return out,
        };
        let (elem_class, stride, len, data) = match self.array_info(entries) {
            Some(a) => a,
            None => return out,
        };
        let layout = match self.entry_layout(elem_class) {
            Some(l) => l,
            None => return out,
        };

        // `_count` is the used high-water mark; entries past it are
        // uninitialised, so bounding by it is both correct and cheaper.
        let used = self
            .i32_field(dict, "_count")
            .filter(|c| *c >= 0)
            .map(|c| c as usize)
            .unwrap_or(len as usize);
        let count = used.min(len as usize).min(max);

        // One bulk read beats `count` syscalls by orders of magnitude.
        let blob = self.mem.read_bytes(data, count * stride);
        if blob.len() < count * stride {
            return out;
        }

        for i in 0..count {
            let base = i * stride;
            let hash = i32::from_le_bytes(
                blob[base + layout.hash..base + layout.hash + 4]
                    .try_into()
                    .unwrap_or([0; 4]),
            );
            if hash < 0 {
                continue; // free slot
            }
            out.push((
                data + base + layout.key,
                layout.key_code,
                data + base + layout.value,
                layout.value_code,
            ));
        }
        out
    }

    /// Read a `Dictionary<K,V>` into raw `(key_addr, value_addr)` pairs plus the
    /// entry layout, when the caller wants to decode values itself.
    pub fn dict_pairs_bulk(&self, dict: usize, max: usize) -> Option<(Vec<u8>, usize, usize, EntryLayout)> {
        let entries = self.ref_field(dict, "_entries")?;
        let (elem_class, stride, len, data) = self.array_info(entries)?;
        let layout = self.entry_layout(elem_class)?;
        let count = (len as usize).min(max);
        let blob = self.mem.read_bytes(data, count * stride);
        if blob.len() < count * stride {
            return None;
        }
        Some((blob, stride, data, layout))
    }

    /// `(items_data_addr, element_size, size)` for a `List<T>`.
    pub fn list_info(&self, list: usize) -> Option<(usize, usize, i32, usize)> {
        let items = self.ref_field(list, "_items")?;
        let size = self.i32_field(list, "_size")?;
        let (elem_class, elem_size, cap, data) = self.array_info(items)?;
        let size = size.min(cap);
        Some((data, elem_size, size, elem_class))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EntryLayout {
    pub hash: usize,
    pub key: usize,
    pub value: usize,
    pub key_code: u8,
    pub value_code: u8,
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

const MH_MAGIC_64: u32 = 0xFEED_FACF;
const LC_SEGMENT_64: u32 = 0x19;
const LC_ID_DYLIB: u32 = 0x0D;

/// A Mach-O image mapped into the target process.
#[derive(Clone, Debug)]
pub struct Image {
    pub base: usize,
    /// Install name from `LC_ID_DYLIB`, else the path the kernel reports.
    pub name: String,
    /// Writable data segments, `(name, addr, size)`, slid into place.
    pub data_segments: Vec<(String, usize, usize)>,
}

/// Enumerate loaded Mach-O images via dyld's `all_image_infos`.
///
/// This is authoritative: scanning VM regions for the Mach-O magic misses
/// images whose header doesn't land on a region boundary (`mach_vm_region`
/// coalesces adjacent entries that share attributes).
pub fn find_images(mem: &Mem) -> Vec<Image> {
    // (load address, path reported by dyld)
    let mut bases: Vec<(usize, String)> = Vec::new();

    if let Some(all_infos) = mem.dyld_all_image_infos() {
        let count = mem.meta_u32(all_infos + 4) as usize;
        let array = mem.meta_ptr(all_infos + 8);
        if plausible(array) && count <= 4096 {
            for i in 0..count {
                // struct dyld_image_info { header*; path*; modDate; }
                let entry = array + i * 24;
                let base = mem.meta_ptr(entry);
                if plausible(base) {
                    let path = mem.meta_cstr(mem.meta_ptr(entry + 8)).unwrap_or_default();
                    bases.push((base, path));
                }
            }
        }
    }

    // Fallback: scan region starts for the magic.
    if bases.is_empty() {
        for r in mem.regions() {
            if r.prot & 1 != 0 && mem.meta_u32(r.start) == MH_MAGIC_64 {
                bases.push((r.start, String::new()));
            }
        }
    }

    let mut images = Vec::new();

    for (base, dyld_path) in bases {
        if mem.meta_u32(base) != MH_MAGIC_64 {
            continue;
        }

        let ncmds = mem.meta_u32(base + 16);
        if ncmds == 0 || ncmds > 4096 {
            continue;
        }

        let mut off = base + 32;
        let mut name = dyld_path;
        let mut text_vmaddr = 0usize;
        let mut segments: Vec<(String, usize, usize)> = Vec::new();

        for _ in 0..ncmds {
            let cmd = mem.meta_u32(off);
            let cmdsize = mem.meta_u32(off + 4) as usize;
            if cmdsize < 8 || cmdsize > 0x10000 {
                break;
            }

            if cmd == LC_SEGMENT_64 {
                if let Some(nb) = mem.meta_bytes(off + 8, 16) {
                    let end = nb.iter().position(|&b| b == 0).unwrap_or(16);
                    let seg = String::from_utf8_lossy(&nb[..end]).to_string();
                    let vmaddr = mem.meta_ptr(off + 24);
                    let vmsize = mem.meta_ptr(off + 32);
                    if seg == "__TEXT" {
                        text_vmaddr = vmaddr;
                    }
                    segments.push((seg, vmaddr, vmsize));
                }
            } else if cmd == LC_ID_DYLIB && name.is_empty() {
                let str_off = mem.meta_u32(off + 8) as usize;
                if str_off < cmdsize {
                    name = mem.meta_cstr(off + str_off).unwrap_or_default();
                }
            }

            off += cmdsize;
        }

        if name.is_empty() {
            name = libproc::libproc::proc_pid::regionfilename(mem.pid as i32, base as u64)
                .unwrap_or_default();
        }

        let slide = base.wrapping_sub(text_vmaddr);
        let data_segments = segments
            .into_iter()
            .filter(|(s, _, _)| s.starts_with("__DATA"))
            .map(|(s, a, z)| (s, a.wrapping_add(slide), z))
            .collect();

        images.push(Image { base, name, data_segments });
    }

    images
}

/// Writable data segments of GameAssembly.dylib, where the IL2CPP globals live.
fn find_game_assembly_data(mem: &Mem) -> Option<Vec<(usize, usize)>> {
    let images = find_images(mem);
    let img = images
        .iter()
        .find(|i| i.name.contains("GameAssembly"))
        // Fall back to the image with the most data: on some builds IL2CPP is
        // linked straight into the main executable.
        .or_else(|| {
            images.iter().max_by_key(|i| {
                i.data_segments.iter().map(|(_, _, z)| *z).sum::<usize>()
            })
        })?;

    let segs: Vec<(usize, usize)> = img
        .data_segments
        .iter()
        .map(|(_, a, z)| (*a, *z))
        .collect();

    if segs.is_empty() {
        None
    } else {
        Some(segs)
    }
}

/// Does `class` look like an `Il2CppClass` under a given self-pointer anchor?
fn is_class_at(mem: &Mem, class: usize, anchor: usize) -> bool {
    plausible(class) && mem.is_mapped(class + anchor) && mem.meta_ptr(class + anchor) == class
}

/// Scan `__DATA` for `s_TypeInfoTable`: a pointer to an array of
/// `Il2CppClass*`. Validated structurally (via the `klass` self-pointer), so
/// this survives game updates instead of relying on a fixed offset.
///
/// The per-candidate test must stay cheap — a data segment holds ~1M candidate
/// words — so the anchor is assumed, then confirmed once on the winner.
/// `0x78` has been the `klass` slot since Unity 2018; the other anchors are a
/// fallback that only costs anything if the fast path finds nothing.
fn find_type_info_table(
    mem: &Mem,
    data_base: usize,
    data_size: usize,
) -> Option<(usize, usize, ClassLayout, usize)> {
    for anchor in [0x78usize, 0x70, 0x80, 0x68, 0x88, 0x60, 0x90] {
        if let Some((table, table_off, count)) = scan_for_table(mem, data_base, data_size, anchor) {
            // Confirm the layout against real entries from the table we found.
            let samples: Vec<usize> = (0..256)
                .map(|k| mem.meta_ptr(table + k * 8))
                .filter(|c| plausible(*c))
                .take(64)
                .collect();

            let layout = ClassLayout::detect(mem, &samples).unwrap_or_else(|| {
                let mut l = ClassLayout::default();
                if anchor != l.klass_self {
                    let shift = anchor as isize - l.klass_self as isize;
                    let adj = |v: usize| (v as isize + shift) as usize;
                    l.element_class = adj(l.element_class);
                    l.cast_class = adj(l.cast_class);
                    l.declaring_type = adj(l.declaring_type);
                    l.parent = adj(l.parent);
                    l.generic_class = adj(l.generic_class);
                    l.klass_self = anchor;
                    l.fields = anchor + 8;
                    l.static_fields = adj(l.static_fields);
                    l.instance_size = adj(l.instance_size);
                    l.element_size = adj(l.element_size);
                    l.flags = adj(l.flags);
                }
                l
            });

            return Some((table, table_off, layout, count));
        }
    }
    None
}

/// One cheap pass over a data segment looking for the table.
fn scan_for_table(
    mem: &Mem,
    data_base: usize,
    data_size: usize,
    anchor: usize,
) -> Option<(usize, usize, usize)> {
    let size = data_size.min(64 * 1024 * 1024);
    let mut best: Option<(usize, usize, usize)> = None;

    let mut off = 0usize;
    while off < size {
        let chunk_len = PAGE.min(size - off);
        let chunk = match mem.meta_bytes(data_base + off, chunk_len) {
            Some(c) => c,
            None => {
                off += chunk_len;
                continue;
            }
        };

        for (i, w) in chunk.chunks_exact(8).enumerate() {
            let candidate = usize::from_le_bytes(w.try_into().unwrap());
            if !plausible(candidate) || !mem.is_mapped(candidate) {
                continue;
            }
            // Gate on the first entry before paying for anything else.
            if !is_class_at(mem, mem.meta_ptr(candidate), anchor) {
                continue;
            }

            // Count leading entries that are classes. The real table has
            // thousands; incidental pointer pairs have a handful.
            let mut valid = 0usize;
            for k in 0..256usize {
                let c = mem.meta_ptr(candidate + k * 8);
                if c == 0 {
                    continue;
                }
                if is_class_at(mem, c, anchor) {
                    valid += 1;
                } else {
                    break;
                }
            }
            if valid < 64 {
                continue;
            }

            let count = table_length(mem, candidate, anchor);
            if best.as_ref().map(|b| count > b.2).unwrap_or(true) {
                best = Some((candidate, off + i * 8, count));
            }
        }

        off += chunk_len;
    }

    best
}

/// Walk the table until entries stop looking like classes.
fn table_length(mem: &Mem, table: usize, anchor: usize) -> usize {
    let mut len = 0usize;
    let mut miss = 0usize;
    for i in 0..300_000usize {
        let addr = table + i * 8;
        if !mem.is_mapped(addr) {
            break;
        }
        let c = mem.meta_ptr(addr);
        if c == 0 || is_class_at(mem, c, anchor) {
            miss = 0;
            len = i + 1;
        } else {
            miss += 1;
            if miss > 32 {
                break;
            }
        }
    }
    len
}

/// Find the MTGA process id.
pub fn find_pid(process_name: &str) -> Option<u32> {
    // Only processes are needed; `new_all`/`refresh_all` would also enumerate
    // disks, networks and components on every poll.
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes();
    // Prefer an exact name match; fall back to a substring match.
    sys.processes()
        .iter()
        .find(|(_, p)| p.name() == process_name)
        .or_else(|| {
            sys.processes()
                .iter()
                .find(|(_, p)| p.name().contains(process_name))
        })
        .map(|(pid, _)| pid.as_u32())
}
