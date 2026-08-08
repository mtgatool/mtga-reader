#[cfg(target_os = "windows")]
use proc_mem::{ProcMemError, Process};

#[cfg(target_os = "windows")]
use is_elevated::is_elevated;

#[cfg(target_os = "linux")]
use sudo::RunningAs;

use sysinfo::{Pid, System};

use process_memory::{DataMember, Memory, ProcessHandle, TryIntoProcessHandle};

use crate::constants;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use crate::pe_reader::PEReader;

pub struct MonoReader {
    pid: u32,
    handle: ProcessHandle,
    mono_root_domain: usize,
    assembly_image_address: usize,
}

impl MonoReader {
    /// Open a handle to the target process.
    ///
    /// Returns an error instead of panicking when the process handle cannot be
    /// acquired. On Windows this most commonly fails with `PermissionDenied`
    /// (OS error 5) when the caller is not elevated but the target (MTGA) is.
    pub fn new(pid: u32) -> std::io::Result<Self> {
        let handle = (pid as process_memory::Pid).try_into_process_handle()?;

        Ok(MonoReader {
            pid,
            handle,
            mono_root_domain: 0,
            assembly_image_address: 0,
        })
    }

    /// The pid this reader was opened against.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn find_pid_by_name(name: &str) -> Option<Pid> {
        let mut sys = System::new_all();
        sys.refresh_all();

        sys.processes()
            .iter()
            .find(|(_, process)| process.name().contains(name))
            .map(|(pid, _)| *pid)
    }

    pub fn is_admin() -> bool {
        #[cfg(target_os = "windows")]
        {
            return is_elevated();
        }
        #[cfg(target_os = "linux")]
        {
            // Root is only one of three ways to qualify for reading another
            // process' memory. A plain user with CAP_SYS_PTRACE, or any user at
            // all under a permissive Yama policy, can call process_vm_readv
            // just fine — reporting "not elevated" to them is a false alarm.
            return sudo::check() == RunningAs::Root
                || Self::has_cap_sys_ptrace()
                || Self::yama_allows_attach();
        }
        #[cfg(target_os = "macos")]
        {
            // On macOS, check if running as root using std
            // For memory reading, we need debugging entitlements or root
            return std::process::id() == 0 || std::env::var("SUDO_USER").is_ok();
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            return false;
        }
    }

    #[cfg(target_os = "windows")]
    pub fn read_mono_root_domain(&mut self) -> usize {
        let mtga_process = match Process::with_pid(self.pid) {
            Ok(process) => process,
            Err(e) => {
                eprintln!("Error obtaining process data: {:?}", e);
                // Could not open process (permission denied, not found, ...)
                // Return early with 0 so callers can handle missing domain gracefully
                return 0;
            }
        };

        let module = match mtga_process.module(constants::MONO_LIBRARY) {
            Ok(module) => module,
            Err(ProcMemError::ModuleNotFound) => {
                eprintln!("Mono module not found in process");
                return 0;
            }
            Err(e) => {
                eprintln!("Error obtaining mono dll: {:?}", e);
                return 0;
            }
        };

        // println!("mono-2.0-bdwgc.dll Base addr: {:x?}", module.base_address());

        let pe = PEReader::new(&self, module.base_address() as usize);

        let mono_root_offset = pe.get_function_offset("mono_get_root_domain");

        println!("mono_get_root_domain offset: {:?}", mono_root_offset);

        match mono_root_offset {
            Ok(offset) => {
                self.mono_root_domain = module.base_address() as usize + offset as usize;
            }
            _ => {
                eprintln!("Error: mono_get_root_domain not found");
            }
        }

        println!("mono_root_domain addr: {:x?}", self.mono_root_domain);
        self.mono_root_domain
    }

    /// Whether this process holds CAP_SYS_PTRACE, which grants cross-process
    /// memory reads without being root.
    #[cfg(target_os = "linux")]
    fn has_cap_sys_ptrace() -> bool {
        // CapEff is a hex bitmask in /proc/self/status; CAP_SYS_PTRACE is bit 19.
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status
                    .lines()
                    .find_map(|line| line.strip_prefix("CapEff:"))
                    .and_then(|caps| u64::from_str_radix(caps.trim(), 16).ok())
            })
            .map(|caps| caps & (1 << 19) != 0)
            .unwrap_or(false)
    }

    /// Whether the Yama LSM permits attaching to a non-descendant process.
    ///
    /// Scope 0 is unrestricted same-uid access. Scope 1 (the common default)
    /// limits attachment to descendants, and MTGA is never our child, so
    /// anything above 0 means we cannot read it as an ordinary user.
    #[cfg(target_os = "linux")]
    fn yama_allows_attach() -> bool {
        match std::fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope") {
            // No Yama at all: classic ptrace rules, same-uid is sufficient.
            Err(_) => true,
            Ok(scope) => scope.trim() == "0",
        }
    }

    /// Locate the Mono runtime's PE image base in a Wine/Proton-hosted MTGA.
    ///
    /// Wine maps the game's DLLs from disk, so `/proc/<pid>/maps` names them
    /// outright — the same information `Process::module()` hands the Windows
    /// path. Sweeping memory for the `MZ` magic is not a workable substitute
    /// here: Wine loads the image up around 0x6ffff_xxxx_xxxx, which is ~3e10
    /// pages above zero, so a 4KiB-stride walk never arrives.
    /// Pick the Mono image base out of the text of `/proc/<pid>/maps`.
    ///
    /// Split from the file read so it can be tested off Linux: the whole of the
    /// logic is string parsing, and the only thing the syscall adds is the
    /// string. Feeding it a captured maps file is a far better test than a VM.
    ///
    /// Not gated on the platform for the same reason.
    pub fn parse_mono_base(maps: &str) -> Option<usize> {
        maps.lines()
            .filter(|line| line.ends_with(constants::MONO_LIBRARY))
            .filter_map(|line| {
                // start-end perms file_offset dev inode path
                let mut fields = line.split_whitespace();
                let start = fields.next()?.split('-').next()?;
                let file_offset = fields.nth(1)?;

                Some((
                    usize::from_str_radix(file_offset, 16).ok()?,
                    usize::from_str_radix(start, 16).ok()?,
                ))
            })
            // The mapping at file offset 0 is the one carrying the PE header.
            .min()
            .map(|(_, start)| start)
    }

    /// Locate the Mono runtime's PE image base in a Wine/Proton-hosted MTGA.
    ///
    /// Wine maps the game's DLLs from disk, so `/proc/<pid>/maps` names them
    /// outright — the same information `Process::module()` hands the Windows
    /// path. Sweeping memory for the `MZ` magic is not a workable substitute
    /// here: Wine loads the image up around 0x6ffff_xxxx_xxxx, which is ~3e10
    /// pages above zero, so a 4KiB-stride walk never arrives.
    #[cfg(target_os = "linux")]
    fn mono_module_base(pid: u32) -> Option<usize> {
        let maps = std::fs::read_to_string(format!("/proc/{}/maps", pid)).ok()?;
        Self::parse_mono_base(&maps)
    }

    #[cfg(target_os = "linux")]
    pub fn read_mono_root_domain(&mut self) -> usize {
        let base = match Self::mono_module_base(self.pid) {
            Some(base) => base,
            None => {
                eprintln!("Mono module not found in process maps");
                return 0;
            }
        };

        let pe = PEReader::new(&self, base);

        match pe.get_function_offset("mono_get_root_domain") {
            Ok(offset) => {
                println!("mono_get_root_domain offset: {:?}", offset);
                self.mono_root_domain = base + offset as usize;
            }
            _ => {
                eprintln!("Error: mono_get_root_domain not found");
            }
        }

        println!("mono_root_domain addr: {:x?}", self.mono_root_domain);
        self.mono_root_domain
    }

    #[cfg(target_os = "macos")]
    pub fn read_mono_root_domain(&mut self) -> usize {
        self.mono_root_domain = 0 as usize;

        self.mono_root_domain
    }

    pub fn create_type_definitions(&mut self) -> Vec<usize> {
        self.create_type_definitions_for_image(self.assembly_image_address)
    }

    pub fn create_type_definitions_for_image(&self, assembly_image_addr: usize) -> Vec<usize> {
        let class_cache_size = self.read_u32(
            assembly_image_addr
                + (constants::IMAGE_CLASS_CACHE + constants::HASH_TABLE_SIZE) as usize,
        );
        let class_cache_table_array = self.read_ptr(
            assembly_image_addr
                + (constants::IMAGE_CLASS_CACHE + constants::HASH_TABLE_TABLE) as usize,
        );

        let mut table_item = 0;
        let mut type_defs: Vec<usize> = Vec::new();

        while table_item < (class_cache_size * constants::SIZE_OF_PTR as u32) {
            let mut definition = self.read_ptr(class_cache_table_array + table_item as usize);

            // Walk the linked list of classes in this hash bucket
            while definition != 0 {
                // Add this definition BEFORE moving to next
                type_defs.push(definition);
                // Follow the next_class_cache pointer to the next entry in the chain
                definition = self
                    .read_ptr(definition + constants::TYPE_DEFINITION_NEXT_CLASS_CACHE as usize);
            }

            table_item += constants::SIZE_OF_PTR as u32;
        }

        return type_defs;
    }

    /// Get all loaded assembly names
    pub fn get_all_assembly_names(&mut self) -> Vec<String> {
        let mut assemblies = Vec::new();
        
        let offset = self.read_i32(self.mono_root_domain + constants::RIP_PLUS_OFFSET_OFFSET)
            + constants::RIP_VALUE_OFFSET as i32;
        let domain = self.read_ptr(self.mono_root_domain + offset as usize);
        let assembly_array_address =
            self.read_ptr(domain + constants::REFERENCED_ASSEMBLIES as usize);

        let mut assembly_address = assembly_array_address;

        while assembly_address != 0 {
            let assembly = self.read_ptr(assembly_address);
            let assembly_name_address =
                self.read_ptr(assembly + (constants::SIZE_OF_PTR * 2 as usize));
            
            if let Some(name) = self.maybe_read_ascii_string(assembly_name_address) {
                if !name.is_empty() {
                    assemblies.push(name);
                }
            }
            assembly_address = self.read_ptr(assembly_address + constants::SIZE_OF_PTR as usize);
        }
        
        assemblies
    }
    
    /// Read assembly image by name
    pub fn read_assembly_image_by_name(&mut self, target_name: &str) -> usize {
        let offset = self.read_i32(self.mono_root_domain + constants::RIP_PLUS_OFFSET_OFFSET)
            + constants::RIP_VALUE_OFFSET as i32;
        let domain = self.read_ptr(self.mono_root_domain + offset as usize);
        let assembly_array_address =
            self.read_ptr(domain + constants::REFERENCED_ASSEMBLIES as usize);

        let mut assembly_address = assembly_array_address;

        while assembly_address != 0 {
            let assembly = self.read_ptr(assembly_address);
            let assembly_name_address =
                self.read_ptr(assembly + (constants::SIZE_OF_PTR * 2 as usize));

            if let Some(assembly_name) = self.maybe_read_ascii_string(assembly_name_address) {
                if assembly_name == target_name {
                    self.assembly_image_address =
                        self.read_ptr(assembly + constants::ASSEMBLY_IMAGE as usize);
                    return self.assembly_image_address;
                }
            }
            assembly_address = self.read_ptr(assembly_address + constants::SIZE_OF_PTR as usize);
        }
        
        0
    }
    
    pub fn read_assembly_image(&mut self) -> usize {
        let offset = self.read_i32(self.mono_root_domain + constants::RIP_PLUS_OFFSET_OFFSET)
            + constants::RIP_VALUE_OFFSET as i32;

        println!("offset: {:?}", offset);

        let domain = self.read_ptr(self.mono_root_domain + offset as usize);

        println!("Domain address: {:x?}", domain);

        let assembly_array_address =
            self.read_ptr(domain + constants::REFERENCED_ASSEMBLIES as usize);

        let mut assembly_address = assembly_array_address;

        while assembly_address != 0 {
            let assembly = self.read_ptr(assembly_address);

            let assembly_name_address =
                self.read_ptr(assembly + (constants::SIZE_OF_PTR * 2 as usize));

            let maybe_name = self.maybe_read_ascii_string(assembly_name_address);

            match maybe_name {
                Some(assembly_name) => {
                    if assembly_name == "Assembly-CSharp" {
                        println!("Assembly name: {:?}", assembly_name);
                        println!("  - {:?}", assembly_name_address);
                        self.assembly_image_address =
                            self.read_ptr(assembly + constants::ASSEMBLY_IMAGE as usize);
                        return self.assembly_image_address;
                    }
                }
                None => {
                    eprintln!("Error reading assembly name");
                }
            }

            assembly_address = self.read_ptr(assembly_address + constants::SIZE_OF_PTR as usize);
        }

        return self.assembly_image_address;
    }

    pub fn maybe_read_u8(&self, addr: usize) -> Option<u8> {
        let mut member = DataMember::<u8>::new(self.handle);

        member.set_offset(vec![addr as usize]);

        let val = unsafe {
            match member.read() {
                Ok(val) => Some(val),
                Err(_e) => None,
            }
        };

        return val;
    }

    pub fn read_u8(&self, addr: usize) -> u8 {
        self.maybe_read_u8(addr).unwrap_or(0)
    }

    pub fn read_u16(&self, addr: usize) -> u16 {
        let mut member = DataMember::<u16>::new(self.handle);
        member.set_offset(vec![addr as usize]);
        unsafe { member.read().unwrap_or(0) }
    }

    // All read_ methods should be wrapping a maybe_ method
    // Ideally we should only use the maybe_read_ methods
    pub fn maybe_read_u32(&self, addr: usize) -> Option<u32> {
        let mut member = DataMember::<u32>::new(self.handle);

        member.set_offset(vec![addr as usize]);

        let val = unsafe {
            match member.read() {
                Ok(val) => Some(val),
                Err(_e) => None,
            }
        };

        return val;
    }

    pub fn read_u32(&self, addr: usize) -> u32 {
        self.maybe_read_u32(addr).unwrap_or(0)
    }

    pub fn read_u64(&self, addr: usize) -> u64 {
        let mut member = DataMember::<u64>::new(self.handle);
        member.set_offset(vec![addr as usize]);
        unsafe { member.read().unwrap_or(0) }
    }

    pub fn read_i8(&self, addr: usize) -> i8 {
        let mut member = DataMember::<i8>::new(self.handle);
        member.set_offset(vec![addr as usize]);
        unsafe { member.read().unwrap_or(0) }
    }

    pub fn read_i16(&self, addr: usize) -> i16 {
        let mut member = DataMember::<i16>::new(self.handle);
        member.set_offset(vec![addr as usize]);
        unsafe { member.read().unwrap_or(0) }
    }

    pub fn read_i32(&self, addr: usize) -> i32 {
        let mut member = DataMember::<i32>::new(self.handle);
        member.set_offset(vec![addr as usize]);
        unsafe { member.read().unwrap_or(0) }
    }

    pub fn read_i64(&self, addr: usize) -> i64 {
        let mut member = DataMember::<i64>::new(self.handle);
        member.set_offset(vec![addr as usize]);
        unsafe { member.read().unwrap_or(0) }
    }

    pub fn read_f32(&self, addr: usize) -> f32 {
        let mut member = DataMember::<f32>::new(self.handle);
        member.set_offset(vec![addr as usize]);
        unsafe { member.read().unwrap_or(0.0) }
    }

    pub fn read_f64(&self, addr: usize) -> f64 {
        let mut member = DataMember::<f64>::new(self.handle);
        member.set_offset(vec![addr as usize]);
        unsafe { member.read().unwrap_or(0.0) }
    }

    pub fn read_ptr(&self, addr: usize) -> usize {
        let mut member = DataMember::<usize>::new(self.handle);
        member.set_offset(vec![addr as usize]);
        unsafe { member.read().unwrap_or(0) }
    }

    pub fn read_bytes(&self, addr: usize, size: usize) -> Vec<u8> {
        let mut result = vec![0u8; size];
        for i in 0..size {
            result[i] = self.read_u8(addr + i);
        }
        result
    }

    // This methos will throw and error if the address is not readable
    pub fn maybe_read_ascii_string(&self, addr: usize) -> Option<String> {
        let mut string = String::new();
        let mut index = 0;
        loop {
            let val = self.maybe_read_u8(addr + index);
            match val {
                Some(val) => {
                    if val == 0 || index > 1024 {
                        break;
                    }
                    string.push(val as char);
                    index += 1;
                }
                None => break,
            }
        }
        Some(string)
    }

    // This method is optimistic, and will return a cutted string if the address
    // is not readable
    pub fn read_ascii_string(&self, addr: usize) -> String {
        let mut string = String::new();
        let mut index = 0;
        loop {
            let val = self.maybe_read_u8(addr + index);
            match val {
                Some(val) => {
                    if val == 0 || index > 1024 {
                        break;
                    }
                    string.push(val as char);
                    index += 1;
                }
                None => break,
            }
        }
        string
    }

    pub fn read_ptr_u8(&self, addr: usize) -> u8 {
        let ptr = self.read_ptr(addr);
        self.read_u8(ptr)
    }

    pub fn read_ptr_u16(&self, addr: usize) -> u16 {
        let ptr = self.read_ptr(addr);
        self.read_u16(ptr)
    }

    pub fn read_ptr_u32(&self, addr: usize) -> u32 {
        let ptr = self.read_ptr(addr);
        self.read_u32(ptr)
    }

    pub fn read_ptr_u64(&self, addr: usize) -> u64 {
        let ptr = self.read_ptr(addr);
        self.read_u64(ptr)
    }

    pub fn read_ptr_i8(&self, addr: usize) -> i8 {
        let ptr = self.read_ptr(addr);
        self.read_i8(ptr)
    }

    pub fn read_ptr_i16(&self, addr: usize) -> i16 {
        let ptr = self.read_ptr(addr);
        self.read_i16(ptr)
    }

    pub fn read_ptr_i32(&self, addr: usize) -> i32 {
        let ptr = self.read_ptr(addr);
        self.read_i32(ptr)
    }

    pub fn read_ptr_i64(&self, addr: usize) -> i64 {
        let ptr = self.read_ptr(addr);
        self.read_i64(ptr)
    }

    pub fn read_ptr_ascii_string(&self, addr: usize) -> String {
        let ptr = self.read_ptr(addr);
        self.read_ascii_string(ptr)
    }

    /// Read a .NET/Mono String object properly
    /// MonoString structure:
    /// - VTable pointer (SIZE_OF_PTR bytes)
    /// - Monitor/sync block (SIZE_OF_PTR bytes)
    /// - Length (4 bytes, u32)
    /// - Character data (UTF-16, 2 bytes per char)
    pub fn read_mono_string(&self, string_ptr: usize) -> Option<String> {
        if string_ptr == 0 || string_ptr < 0x10000 {
            return None;
        }

        // Read string length from MonoString header
        let length = self.read_u32(string_ptr + (constants::SIZE_OF_PTR * 2));

        if length == 0 || length > 10000 {
            return None; // Sanity check: reject empty or unreasonably long strings
        }

        // Read UTF-16 characters
        let mut utf16_chars = Vec::new();
        let chars_offset = string_ptr + (constants::SIZE_OF_PTR * 2) + 4;

        for i in 0..length {
            let char_val = self.read_u16(chars_offset + (i as usize * 2));
            utf16_chars.push(char_val);
        }

        // Decode UTF-16 to Rust String
        String::from_utf16(&utf16_chars).ok()
    }

    /// Read a .NET string by first dereferencing a pointer to the string object
    pub fn read_ptr_mono_string(&self, addr: usize) -> Option<String> {
        let string_ptr = self.read_ptr(addr);
        self.read_mono_string(string_ptr)
    }

    pub fn read_ptr_ptr(&self, addr: usize) -> usize {
        let ptr = self.read_ptr(addr);
        self.read_ptr(ptr)
    }
}

#[cfg(test)]
mod tests {
    use super::MonoReader;

    /// A Wine/Proton MTGA, trimmed. The runtime is mapped in several pieces —
    /// the PE header, then the executable and data sections — and only the one
    /// at file offset 0 is the image base.
    const WINE_MAPS: &str = "\
00400000-00401000 r--p 00000000 08:02 100 /home/u/.steam/steamapps/common/MTGA/MTGA.exe
7f3c1c000000-7f3c1c021000 rw-p 00000000 00:00 0
6ffffa010000-6ffffa011000 r--p 00000000 08:02 987 /home/u/.steam/steamapps/common/MTGA/MTGA_Data/MonoBleedingEdge/EmbedRuntime/mono-2.0-bdwgc.dll
6ffffa011000-6ffffa1a0000 r-xp 00001000 08:02 987 /home/u/.steam/steamapps/common/MTGA/MTGA_Data/MonoBleedingEdge/EmbedRuntime/mono-2.0-bdwgc.dll
6ffffa1a0000-6ffffa1f0000 r--p 00190000 08:02 987 /home/u/.steam/steamapps/common/MTGA/MTGA_Data/MonoBleedingEdge/EmbedRuntime/mono-2.0-bdwgc.dll
7ffff7000000-7ffff7028000 r-xp 00000000 08:02 555 /usr/lib/libc.so.6";

    #[test]
    fn finds_the_image_base_among_several_mappings() {
        // Not the lowest address and not the first line — the one at file
        // offset 0, which is where the PE header lives.
        assert_eq!(MonoReader::parse_mono_base(WINE_MAPS), Some(0x6ffffa010000));
    }

    /// The rule is "the mapping at file offset 0", not "the lowest address".
    /// They coincide in a real maps file, which is sorted, so this pins the
    /// intent against a future rewrite that reaches for the address instead.
    #[test]
    fn picks_the_header_mapping_not_the_lowest_address() {
        let maps = "\
6ffffa000000-6ffffa010000 r-xp 00001000 08:02 987 /x/mono-2.0-bdwgc.dll
6ffffa010000-6ffffa011000 r--p 00000000 08:02 987 /x/mono-2.0-bdwgc.dll";
        assert_eq!(MonoReader::parse_mono_base(maps), Some(0x6ffffa010000));
    }

    #[test]
    fn ignores_a_process_without_mono() {
        let maps = "00400000-00401000 r--p 00000000 08:02 100 /usr/bin/bash\n\
                    7ffff7000000-7ffff7028000 r-xp 00000000 08:02 555 /usr/lib/libc.so.6";
        assert_eq!(MonoReader::parse_mono_base(maps), None);
    }

    #[test]
    fn survives_an_anonymous_mapping_with_no_path() {
        // A line with fewer fields must not take the parser down with it.
        let maps = "7f3c1c000000-7f3c1c021000 rw-p 00000000 00:00 0\n\
                    6ffffa010000-6ffffa011000 r--p 00000000 08:02 987 /x/mono-2.0-bdwgc.dll";
        assert_eq!(MonoReader::parse_mono_base(maps), Some(0x6ffffa010000));
    }

    /// Arena updating while it runs leaves its mappings tagged `(deleted)`, and
    /// the suffix match then fails. Documents today's behaviour rather than
    /// endorsing it — the read gives up where the old scan would have hung.
    #[test]
    fn a_deleted_mapping_is_not_matched() {
        let maps = "6ffffa010000-6ffffa011000 r--p 00000000 08:02 987 \
                    /x/mono-2.0-bdwgc.dll (deleted)";
        assert_eq!(MonoReader::parse_mono_base(maps), None);
    }
}
