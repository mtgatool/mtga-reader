//! IL2CPP Offset Testing Tool
//!
//! This tool scans a running IL2CPP game process to find the correct
//! structure offsets. Run with: sudo cargo run --bin test_il2cpp_offsets

use std::fs::File;
use std::io::Write;
use std::process::Command;

// Minimal memory reader using mach APIs
struct MemReader {
    task_port: u32,
}

impl MemReader {
    fn new(pid: u32) -> Self {
        let task_port = unsafe {
            let mut task: u32 = 0;
            mach2::traps::task_for_pid(mach2::traps::mach_task_self(), pid as i32, &mut task);
            task
        };
        MemReader { task_port }
    }

    fn read_bytes(&self, addr: usize, size: usize) -> Vec<u8> {
        let mut buffer = vec![0u8; size];
        let mut out_size: u64 = 0;
        unsafe {
            mach2::vm::mach_vm_read_overwrite(
                self.task_port,
                addr as u64,
                size as u64,
                buffer.as_mut_ptr() as u64,
                &mut out_size,
            );
        }
        buffer.truncate(out_size as usize);
        buffer
    }

    fn read_ptr(&self, addr: usize) -> usize {
        let bytes = self.read_bytes(addr, 8);
        if bytes.len() < 8 {
            return 0;
        }
        usize::from_le_bytes(bytes[0..8].try_into().unwrap_or([0; 8]))
    }

    fn read_u32(&self, addr: usize) -> u32 {
        let bytes = self.read_bytes(addr, 4);
        if bytes.len() < 4 {
            return 0;
        }
        u32::from_le_bytes(bytes[0..4].try_into().unwrap_or([0; 4]))
    }

    fn read_i32(&self, addr: usize) -> i32 {
        let bytes = self.read_bytes(addr, 4);
        if bytes.len() < 4 {
            return 0;
        }
        i32::from_le_bytes(bytes[0..4].try_into().unwrap_or([0; 4]))
    }

    fn read_string(&self, addr: usize) -> String {
        let bytes = self.read_bytes(addr, 256);
        if let Some(null_pos) = bytes.iter().position(|&b| b == 0) {
            String::from_utf8_lossy(&bytes[0..null_pos]).into_owned()
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        }
    }
}

fn find_mtga_process() -> Option<u32> {
    let output = Command::new("pgrep")
        .arg("-i")
        .arg("mtga")
        .output()
        .ok()?;

    let pid: u32 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .lines()
        .next()?
        .parse()
        .ok()?;

    Some(pid)
}

fn find_second_data_segment(pid: u32) -> usize {
    let output = Command::new("vmmap")
        .args(["-wide", &pid.to_string()])
        .output()
        .expect("vmmap failed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut found_first = false;

    for line in stdout.lines() {
        if line.contains("GameAssembly") && line.contains("__DATA") && !line.contains("__DATA_CONST") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let addr_parts: Vec<&str> = parts[1].split('-').collect();
                if let Ok(start) = usize::from_str_radix(addr_parts[0], 16) {
                    if found_first {
                        return start;
                    }
                    found_first = true;
                }
            }
        }
    }
    0
}

fn find_type_info_table(reader: &MemReader, data_base: usize) -> Option<usize> {
    println!("Scanning for type info table...");

    // Try known offsets first
    let known_offsets = [0x24360, 0x24350, 0x24370, 0x24340, 0x24380, 0x243A0, 0x243C0, 0x24400];
    for offset in known_offsets {
        if let Some(addr) = data_base.checked_add(offset) {
            let table = reader.read_ptr(addr);
            if table > 0x100000 && table < 0x400000000 {
                // Validate by checking a few entries
                let mut valid_count = 0;
                let mut sample_names = Vec::new();

                for i in 0..30 {
                    if let Some(entry_addr) = table.checked_add(i * 8) {
                        let class_ptr = reader.read_ptr(entry_addr);
                        if class_ptr > 0x100000 && class_ptr < 0x400000000 {
                            if let Some(name_addr) = class_ptr.checked_add(0x10) {
                                let name_ptr = reader.read_ptr(name_addr);
                                if name_ptr > 0 && name_ptr < 0x400000000 {
                                    let name = reader.read_string(name_ptr);
                                    if !name.is_empty() && name.len() < 100 && name.chars().all(|c| c.is_ascii_graphic() || c == '_') {
                                        valid_count += 1;
                                        if sample_names.len() < 3 {
                                            sample_names.push(name);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                println!("  Offset 0x{:x} -> table 0x{:x}: {} valid entries", offset, table, valid_count);
                if !sample_names.is_empty() {
                    println!("    Sample names: {:?}", sample_names);
                }

                if valid_count >= 10 {
                    println!("  ✓ Found valid table at offset 0x{:x}", offset);
                    return Some(table);
                }
            }
        }
    }

    // If not found, scan the first 256KB of the DATA segment
    println!("Known offsets failed. Scanning DATA segment (this may take a moment)...");
    for offset in (0..0x40000).step_by(8) {
        if let Some(addr) = data_base.checked_add(offset) {
            let table = reader.read_ptr(addr);

            if table > 0x100000 && table < 0x400000000 {
                let mut valid_count = 0;
                let mut sample_names = Vec::new();

                for i in 0..30 {
                    if let Some(entry_addr) = table.checked_add(i * 8) {
                        let class_ptr = reader.read_ptr(entry_addr);
                        if class_ptr > 0x100000 && class_ptr < 0x400000000 {
                            if let Some(name_addr) = class_ptr.checked_add(0x10) {
                                let name_ptr = reader.read_ptr(name_addr);
                                if name_ptr > 0 && name_ptr < 0x400000000 {
                                    let name = reader.read_string(name_ptr);
                                    if !name.is_empty() && name.len() < 100 && name.chars().all(|c| c.is_ascii_graphic() || c == '_') {
                                        valid_count += 1;
                                        if sample_names.len() < 3 {
                                            sample_names.push(name);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if valid_count >= 10 {
                    println!("  ✓ Found potential table at offset 0x{:x} -> 0x{:x} ({} valid entries)", offset, table, valid_count);
                    println!("    Sample classes: {:?}", sample_names);
                    return Some(table);
                }
            }
        }
    }

    None
}

fn find_class_by_name(reader: &MemReader, type_info_table: usize, name: &str) -> Option<usize> {
    for i in 0..50000 {
        if let Some(entry_addr) = type_info_table.checked_add(i * 8) {
            let class_ptr = reader.read_ptr(entry_addr);
            if class_ptr == 0 || class_ptr < 0x100000 {
                continue;
            }
            if let Some(name_addr) = class_ptr.checked_add(0x10) {
                let name_ptr = reader.read_ptr(name_addr);
                if name_ptr > 0 {
                    let class_name = reader.read_string(name_ptr);
                    if class_name == name {
                        return Some(class_ptr);
                    }
                }
            }
        }
    }
    None
}

fn test_class_offsets(reader: &MemReader, class_addr: usize, log: &mut File) {
    writeln!(log, "\n=== Testing Class Structure at 0x{:x} ===\n", class_addr).unwrap();

    // Test field count offsets
    writeln!(log, "Field Count Tests:").unwrap();
    for offset in [0x11C, 0x120, 0x124, 0x128] {
        let count = reader.read_i32(class_addr + offset);
        writeln!(log, "  @ 0x{:03x}: {} ({})", offset, count,
                 if count > 0 && count < 200 { "reasonable" } else { "unlikely" }).unwrap();
    }

    // Test fields pointer offsets
    writeln!(log, "\nFields Pointer Tests:").unwrap();
    for offset in (0x60..=0xC0).step_by(8) {
        let fields_ptr = reader.read_ptr(class_addr + offset);
        if fields_ptr == 0 {
            continue;
        }

        write!(log, "  @ 0x{:03x}: 0x{:x}", offset, fields_ptr).unwrap();

        if fields_ptr > 0x100000 && fields_ptr < 0x400000000 {
            // Try to read first field
            let first_name_ptr = reader.read_ptr(fields_ptr);
            if first_name_ptr > 0x100000 && first_name_ptr < 0x400000000 {
                let name = reader.read_string(first_name_ptr);
                if !name.is_empty() && name.len() < 100 {
                    let is_valid = name.chars().all(|c| c.is_ascii_graphic() || c == '_' || c == '<' || c == '>');
                    if is_valid {
                        writeln!(log, " -> ✓ VALID! First field: '{}'", name).unwrap();

                        // Read more fields to confirm
                        writeln!(log, "     Fields found:").unwrap();
                        for i in 0..10 {
                            if let Some(field_addr) = fields_ptr.checked_add(i * 0x20) {
                                let name_ptr = reader.read_ptr(field_addr);
                                if name_ptr > 0x100000 {
                                    let fname = reader.read_string(name_ptr);
                                    if !fname.is_empty() && fname.chars().all(|c| c.is_ascii_graphic() || c == '_' || c == '<' || c == '>') {
                                        writeln!(log, "       [{}] {}", i, fname).unwrap();
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }
                    } else {
                        writeln!(log, " -> Invalid name: {:?}", name).unwrap();
                    }
                } else {
                    writeln!(log, " -> Empty/long name").unwrap();
                }
            } else {
                writeln!(log, " -> Invalid name_ptr: 0x{:x}", first_name_ptr).unwrap();
            }
        } else {
            writeln!(log, " -> Out of range").unwrap();
        }
    }

    // Test static fields pointer
    writeln!(log, "\nStatic Fields Pointer Tests:").unwrap();
    for offset in [0xA0, 0xA8, 0xB0, 0xB8, 0xC0] {
        let static_ptr = reader.read_ptr(class_addr + offset);
        writeln!(log, "  @ 0x{:03x}: 0x{:x}", offset, static_ptr).unwrap();
    }
}

fn main() {
    println!("IL2CPP Offset Testing Tool");
    println!("===========================\n");

    // Find MTGA process
    let pid = match find_mtga_process() {
        Some(p) => p,
        None => {
            eprintln!("Error: MTGA process not found");
            std::process::exit(1);
        }
    };
    println!("Found MTGA process: PID {}", pid);

    let reader = MemReader::new(pid);

    // Find data segment
    let data_base = find_second_data_segment(pid);
    if data_base == 0 {
        eprintln!("Error: Could not find GameAssembly __DATA segment");
        std::process::exit(1);
    }
    println!("Data segment base: 0x{:x}", data_base);

    // Find type info table
    let type_info_table = match find_type_info_table(&reader, data_base) {
        Some(t) => t,
        None => {
            eprintln!("Error: Could not find type info table");
            std::process::exit(1);
        }
    };
    println!("Type info table: 0x{:x}", type_info_table);

    // Find PAPA class
    let papa_class = match find_class_by_name(&reader, type_info_table, "PAPA") {
        Some(c) => c,
        None => {
            eprintln!("Error: PAPA class not found");
            std::process::exit(1);
        }
    };
    println!("PAPA class: 0x{:x}", papa_class);

    // Create log file in the project directory
    let log_path = std::env::current_dir()
        .unwrap()
        .join("il2cpp_offset_test.log");

    println!("Writing results to: {}", log_path.display());

    let mut log = File::create(&log_path)
        .unwrap_or_else(|e| {
            eprintln!("Failed to create log file at {:?}: {}", log_path, e);
            eprintln!("Writing to stdout instead...");
            std::process::exit(1);
        });

    writeln!(log, "IL2CPP Offset Test Results").unwrap();
    writeln!(log, "==========================").unwrap();
    writeln!(log, "PID: {}", pid).unwrap();
    writeln!(log, "Data Base: 0x{:x}", data_base).unwrap();
    writeln!(log, "Type Info Table: 0x{:x}", type_info_table).unwrap();
    writeln!(log, "PAPA Class: 0x{:x}", papa_class).unwrap();

    // Test offsets
    test_class_offsets(&reader, papa_class, &mut log);

    // Also test a few other classes
    for class_name in ["WrapperController", "InventoryManager"] {
        if let Some(class_addr) = find_class_by_name(&reader, type_info_table, class_name) {
            writeln!(log, "\n\n").unwrap();
            writeln!(log, "=== {} Class: 0x{:x} ===", class_name, class_addr).unwrap();
            test_class_offsets(&reader, class_addr, &mut log);
        }
    }

    println!("\n✓ Results written to {}", log_path.display());
    println!("View with: cat {}", log_path.display());
}
