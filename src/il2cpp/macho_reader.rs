//! Mach-O binary parser for macOS IL2CPP games
//!
//! This module handles parsing Mach-O binaries to find IL2CPP structures
//! in macOS/iOS Unity games.

use crate::backend::MemoryReader;

/// Mach-O magic numbers
const MH_MAGIC_64: u32 = 0xFEEDFACF;
const MH_CIGAM_64: u32 = 0xCFFAEDFE; // Byte-swapped

/// Load command types
const LC_SEGMENT_64: u32 = 0x19;
#[allow(dead_code)]
const LC_SYMTAB: u32 = 0x02;
#[allow(dead_code)]
const LC_DYSYMTAB: u32 = 0x0B;
#[allow(dead_code)]
const LC_LOAD_DYLIB: u32 = 0x0C;

/// Mach-O 64-bit header
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MachHeader64 {
    pub magic: u32,
    pub cputype: i32,
    pub cpusubtype: i32,
    pub filetype: u32,
    pub ncmds: u32,
    pub sizeofcmds: u32,
    pub flags: u32,
    pub reserved: u32,
}

/// Load command header
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LoadCommand {
    pub cmd: u32,
    pub cmdsize: u32,
}

/// Segment command 64
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SegmentCommand64 {
    pub cmd: u32,
    pub cmdsize: u32,
    pub segname: [u8; 16],
    pub vmaddr: u64,
    pub vmsize: u64,
    pub fileoff: u64,
    pub filesize: u64,
    pub maxprot: i32,
    pub initprot: i32,
    pub nsects: u32,
    pub flags: u32,
}

/// Section 64
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Section64 {
    pub sectname: [u8; 16],
    pub segname: [u8; 16],
    pub addr: u64,
    pub size: u64,
    pub offset: u32,
    pub align: u32,
    pub reloff: u32,
    pub nreloc: u32,
    pub flags: u32,
    pub reserved1: u32,
    pub reserved2: u32,
    pub reserved3: u32,
}

/// Symbol table entry 64
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Nlist64 {
    pub n_strx: u32,
    pub n_type: u8,
    pub n_sect: u8,
    pub n_desc: i16,
    pub n_value: u64,
}

/// Mach-O reader for parsing macOS binaries
pub struct MachOReader<'a, R: MemoryReader> {
    reader: &'a R,
    base_address: usize,
    header: Option<MachHeader64>,
}

impl<'a, R: MemoryReader> MachOReader<'a, R> {
    /// Create a new Mach-O reader at the given base address
    pub fn new(reader: &'a R, base_address: usize) -> Self {
        let mut macho = MachOReader {
            reader,
            base_address,
            header: None,
        };
        macho.parse_header();
        macho
    }

    /// Parse the Mach-O header
    fn parse_header(&mut self) {
        let magic = self.reader.read_u32(self.base_address);

        if magic != MH_MAGIC_64 && magic != MH_CIGAM_64 {
            return;
        }

        let header = MachHeader64 {
            magic,
            cputype: self.reader.read_i32(self.base_address + 4),
            cpusubtype: self.reader.read_i32(self.base_address + 8),
            filetype: self.reader.read_u32(self.base_address + 12),
            ncmds: self.reader.read_u32(self.base_address + 16),
            sizeofcmds: self.reader.read_u32(self.base_address + 20),
            flags: self.reader.read_u32(self.base_address + 24),
            reserved: self.reader.read_u32(self.base_address + 28),
        };

        self.header = Some(header);
    }

    /// Check if this is a valid Mach-O binary
    pub fn is_valid(&self) -> bool {
        self.header.is_some()
    }

    /// Get the Mach-O header
    pub fn header(&self) -> Option<&MachHeader64> {
        self.header.as_ref()
    }

    /// Iterate through load commands
    pub fn iter_load_commands(&self) -> LoadCommandIterator<'_, R> {
        let header = match &self.header {
            Some(h) => h,
            None => return LoadCommandIterator {
                reader: self.reader,
                current_offset: 0,
                remaining: 0,
            },
        };

        LoadCommandIterator {
            reader: self.reader,
            current_offset: self.base_address + std::mem::size_of::<MachHeader64>(),
            remaining: header.ncmds,
        }
    }

    /// Find a segment by name
    pub fn find_segment(&self, name: &str) -> Option<SegmentCommand64> {
        for (cmd, offset) in self.iter_load_commands() {
            if cmd.cmd == LC_SEGMENT_64 {
                let segment = self.read_segment_command(offset);
                let seg_name = segment_name_to_string(&segment.segname);
                if seg_name == name {
                    return Some(segment);
                }
            }
        }
        None
    }

    /// Read a segment command at the given offset
    fn read_segment_command(&self, offset: usize) -> SegmentCommand64 {
        let mut segname = [0u8; 16];
        for i in 0..16 {
            segname[i] = self.reader.read_u8(offset + 8 + i);
        }

        SegmentCommand64 {
            cmd: self.reader.read_u32(offset),
            cmdsize: self.reader.read_u32(offset + 4),
            segname,
            vmaddr: self.reader.read_u64(offset + 24),
            vmsize: self.reader.read_u64(offset + 32),
            fileoff: self.reader.read_u64(offset + 40),
            filesize: self.reader.read_u64(offset + 48),
            maxprot: self.reader.read_i32(offset + 56),
            initprot: self.reader.read_i32(offset + 60),
            nsects: self.reader.read_u32(offset + 64),
            flags: self.reader.read_u32(offset + 68),
        }
    }

    /// Find a section within a segment
    pub fn find_section(&self, segment_name: &str, section_name: &str) -> Option<(u64, u64)> {
        for (cmd, offset) in self.iter_load_commands() {
            if cmd.cmd == LC_SEGMENT_64 {
                let segment = self.read_segment_command(offset);
                let seg_name = segment_name_to_string(&segment.segname);

                if seg_name == segment_name {
                    // Iterate through sections
                    let section_start = offset + std::mem::size_of::<SegmentCommand64>();
                    for i in 0..segment.nsects {
                        let sect_offset = section_start + (i as usize * std::mem::size_of::<Section64>());
                        let section = self.read_section(sect_offset);
                        let sect_name = section_name_to_string(&section.sectname);

                        if sect_name == section_name {
                            return Some((section.addr, section.size));
                        }
                    }
                }
            }
        }
        None
    }

    /// Read a section at the given offset
    fn read_section(&self, offset: usize) -> Section64 {
        let mut sectname = [0u8; 16];
        let mut segname = [0u8; 16];

        for i in 0..16 {
            sectname[i] = self.reader.read_u8(offset + i);
            segname[i] = self.reader.read_u8(offset + 16 + i);
        }

        Section64 {
            sectname,
            segname,
            addr: self.reader.read_u64(offset + 32),
            size: self.reader.read_u64(offset + 40),
            offset: self.reader.read_u32(offset + 48),
            align: self.reader.read_u32(offset + 52),
            reloff: self.reader.read_u32(offset + 56),
            nreloc: self.reader.read_u32(offset + 60),
            flags: self.reader.read_u32(offset + 64),
            reserved1: self.reader.read_u32(offset + 68),
            reserved2: self.reader.read_u32(offset + 72),
            reserved3: self.reader.read_u32(offset + 76),
        }
    }
}

/// Iterator over Mach-O load commands
pub struct LoadCommandIterator<'a, R: MemoryReader> {
    reader: &'a R,
    current_offset: usize,
    remaining: u32,
}

impl<'a, R: MemoryReader> Iterator for LoadCommandIterator<'a, R> {
    type Item = (LoadCommand, usize);

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        let cmd = LoadCommand {
            cmd: self.reader.read_u32(self.current_offset),
            cmdsize: self.reader.read_u32(self.current_offset + 4),
        };

        let offset = self.current_offset;
        self.current_offset += cmd.cmdsize as usize;
        self.remaining -= 1;

        Some((cmd, offset))
    }
}

/// Convert segment name bytes to string
fn segment_name_to_string(name: &[u8; 16]) -> String {
    let end = name.iter().position(|&b| b == 0).unwrap_or(16);
    String::from_utf8_lossy(&name[..end]).to_string()
}

/// Convert section name bytes to string
fn section_name_to_string(name: &[u8; 16]) -> String {
    let end = name.iter().position(|&b| b == 0).unwrap_or(16);
    String::from_utf8_lossy(&name[..end]).to_string()
}
