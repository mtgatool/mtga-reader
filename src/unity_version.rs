use std::fmt;
use std::path::PathBuf;

#[cfg(target_os = "windows")]
use pelite::FileMap;
#[cfg(target_os = "windows")]
use pelite::pe64::{Pe, PeFile};

/// Represents a Unity version with year, version, and subversion components
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnityVersion {
    pub year: u32,
    pub version_within_year: u32,
    pub subversion_within_year: u32,
    pub full_version: String,
}

impl UnityVersion {
    pub fn new(year: u32, version: u32, subversion: u32, full: String) -> Self {
        UnityVersion {
            year,
            version_within_year: version,
            subversion_within_year: subversion,
            full_version: full,
        }
    }

    /// Parse a version string like "2021.3.14f1" or "2021.3.14"
    pub fn parse(version_str: &str) -> Option<Self> {
        // Remove any trailing letters like "f1", "p1", etc.
        let cleaned = version_str
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect::<String>();
        
        let parts: Vec<&str> = cleaned.split('.').collect();
        if parts.len() >= 3 {
            let year = parts[0].parse::<u32>().ok()?;
            let version = parts[1].parse::<u32>().ok()?;
            let subversion = parts[2].parse::<u32>().ok()?;
            Some(UnityVersion::new(year, version, subversion, version_str.to_string()))
        } else if parts.len() == 2 {
            let year = parts[0].parse::<u32>().ok()?;
            let version = parts[1].parse::<u32>().ok()?;
            Some(UnityVersion::new(year, version, 0, version_str.to_string()))
        } else {
            None
        }
    }

    /// Check if this version matches the expected version for Unity 2021.3.14
    pub fn matches_2021_3_14(&self) -> bool {
        self.year == 2021 && self.version_within_year == 3 && self.subversion_within_year >= 14
    }
    
    /// Check if this version is newer than Unity 2021.3.x
    pub fn is_newer_than_2021(&self) -> bool {
        self.year > 2021 || (self.year == 2021 && self.version_within_year > 3)
    }
}

impl fmt::Display for UnityVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.full_version)
    }
}

/// Find the MTGA executable path on Windows
#[cfg(target_os = "windows")]
pub fn find_mtga_executable() -> Option<PathBuf> {
    use sysinfo::System;
    
    let mut sys = System::new_all();
    sys.refresh_all();
    
    for (_pid, process) in sys.processes() {
        if process.name().contains("MTGA") {
            // Get the executable path
            if let Some(path) = process.exe() {
                return Some(path.to_path_buf());
            }
        }
    }
    
    None
}

#[cfg(not(target_os = "windows"))]
pub fn find_mtga_executable() -> Option<PathBuf> {
    None
}

/// Read the Unity version from the MTGA.exe PE file
#[cfg(target_os = "windows")]
pub fn get_unity_version_from_exe(exe_path: &PathBuf) -> Option<UnityVersion> {
    use pelite::resources::version_info::Language;
    
    // Map the file into memory
    let file_map = match FileMap::open(exe_path) {
        Ok(fm) => fm,
        Err(e) => {
            eprintln!("Failed to open PE file: {:?}", e);
            return None;
        }
    };
    
    // Parse as PE64
    let pe = match PeFile::from_bytes(file_map.as_ref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to parse PE file: {:?}", e);
            return None;
        }
    };
    
    // Try to get version info from resources
    if let Ok(resources) = pe.resources() {
        if let Ok(version_info) = resources.version_info() {
            // Get the fixed file info
            if let Some(fixed) = version_info.fixed() {
                // pelite VS_VERSION has named fields: Major, Minor, Build, Patch
                let file_version = fixed.dwFileVersion;
                let major = file_version.Major as u32;
                let minor = file_version.Minor as u32;
                let build = file_version.Build as u32;
                let revision = file_version.Patch as u32;
                
                let version_string = format!("{}.{}.{}.{}", major, minor, build, revision);
                println!("PE File Version: {}", version_string);
                
                // Try to get the string version info for more details
                // Use English (US) language code
                let mut found_version: Option<UnityVersion> = None;
                
                version_info.strings(Language::default(), |key, value| {
                    if key.contains("FileVersion") || key.contains("ProductVersion") {
                        println!("  {}: {}", key, value);
                        if found_version.is_none() {
                            found_version = UnityVersion::parse(value);
                        }
                    }
                });
                
                if found_version.is_some() {
                    return found_version;
                }
                
                // Fall back to fixed version
                return UnityVersion::parse(&version_string);
            }
        }
    }
    
    None
}

#[cfg(not(target_os = "windows"))]
pub fn get_unity_version_from_exe(_exe_path: &PathBuf) -> Option<UnityVersion> {
    None
}

/// Detect Unity version from the running MTGA process
pub fn detect_unity_version() -> Option<UnityVersion> {
    let exe_path = find_mtga_executable()?;
    println!("Found MTGA at: {:?}", exe_path);
    get_unity_version_from_exe(&exe_path)
}

/// Offsets for different Unity versions
#[derive(Debug, Clone)]
pub struct MonoOffsets {
    pub version_name: String,
    pub assembly_image: u32,
    pub referenced_assemblies: u32,
    pub image_class_cache: u32,
    pub hash_table_size: u32,
    pub hash_table_table: u32,
    pub type_definition_field_size: u32,
    pub type_definition_bit_fields: u32,
    pub type_definition_class_kind: u32,
    pub type_definition_parent: u32,
    pub type_definition_nested_in: u32,
    pub type_definition_name: u32,
    pub type_definition_namespace: u32,
    pub type_definition_v_table_size: u32,
    pub type_definition_size: u32,
    pub type_definition_fields: u32,
    pub type_definition_by_val_arg: u32,
    pub type_definition_runtime_info: u32,
    pub type_definition_field_count: u32,
    pub type_definition_next_class_cache: u32,
    pub type_definition_mono_generic_class: u32,
    pub type_definition_generic_container: u32,
    pub type_definition_runtime_info_domain_v_tables: u32,
    pub v_table: u32,
}

impl MonoOffsets {
    /// Offsets for Unity 2022.3.x (EXPERIMENTAL - needs verification)
    /// Based on Unity 2021.3.14 with potential adjustments for 2022
    pub fn unity_2022_3() -> Self {
        // Unity 2022.3 may have different Mono struct layouts
        // These are experimental - some offsets may need adjustment
        MonoOffsets {
            version_name: "Unity 2022.3 (experimental)".to_string(),
            assembly_image: 0x10 + 0x50,        // Same as 2021
            referenced_assemblies: 0xa0,         // Same as 2021
            image_class_cache: 0x4d0,            // Seems to work (163 buckets found)
            hash_table_size: 0xc + 0xc,          // Same as 2021
            hash_table_table: 0x14 + 0xc,        // Same as 2021
            type_definition_field_size: 0x10 + 0x10,
            type_definition_bit_fields: 0x14 + 0xc,
            type_definition_class_kind: 0x1b,
            type_definition_parent: 0x30,
            type_definition_nested_in: 0x38,
            type_definition_name: 0x48,          // Type names are being read correctly
            type_definition_namespace: 0x50,
            type_definition_v_table_size: 0x5C,
            type_definition_size: 0x90,
            type_definition_fields: 0x98,
            type_definition_by_val_arg: 0xB8,
            type_definition_runtime_info: 0xD0,
            type_definition_field_count: 0xEC,   // Adjusted for 2022.3
            type_definition_next_class_cache: 0xF0, // Probing shows 0xF0 finds 326 types vs 203 with 0x108
            type_definition_mono_generic_class: 0xE0,
            type_definition_generic_container: 0x110,
            type_definition_runtime_info_domain_v_tables: 0x8,
            v_table: 0x48,
        }
    }
    
    /// Offsets for Unity 2021.3.14 (current MTGA version as of project creation)
    pub fn unity_2021_3_14() -> Self {
        MonoOffsets {
            version_name: "Unity 2021.3.14".to_string(),
            assembly_image: 0x10 + 0x50,
            referenced_assemblies: 0xa0,
            image_class_cache: 0x4d0,
            hash_table_size: 0xc + 0xc,
            hash_table_table: 0x14 + 0xc,
            type_definition_field_size: 0x10 + 0x10,
            type_definition_bit_fields: 0x14 + 0xc,
            type_definition_class_kind: 0x1b,
            type_definition_parent: 0x30,
            type_definition_nested_in: 0x38,
            type_definition_name: 0x48,
            type_definition_namespace: 0x50,
            type_definition_v_table_size: 0x5C,
            type_definition_size: 0x90,
            type_definition_fields: 0x98,
            type_definition_by_val_arg: 0xB8,
            type_definition_runtime_info: 0x84 + 0x34 + 0x18,
            type_definition_field_count: 0xa4 + 0x34 + 0x18 + 0x10,
            type_definition_next_class_cache: 0xa8 + 0x34 + 0x18 + 0x10 + 0x4,
            type_definition_mono_generic_class: 0x94 + 0x34 + 0x18 + 0x10,
            type_definition_generic_container: 0x110,
            type_definition_runtime_info_domain_v_tables: 0x2 + 0x6,
            v_table: 0x48,
        }
    }
    
    /// Offsets for Unity 2019.4.x / 2020.3.x
    pub fn unity_2019_2020() -> Self {
        MonoOffsets {
            version_name: "Unity 2019.4 / 2020.3".to_string(),
            assembly_image: 0x44 + 0x1c,
            referenced_assemblies: 0x6c + 0x5c,
            image_class_cache: 0x354 + 0x16c,
            hash_table_size: 0xc + 0xc,
            hash_table_table: 0x14 + 0xc,
            type_definition_field_size: 0x10 + 0x10,
            type_definition_bit_fields: 0x14 + 0xc,
            type_definition_class_kind: 0x1e + 0xc,
            type_definition_parent: 0x20 + 0x10,
            type_definition_nested_in: 0x24 + 0x14,
            type_definition_name: 0x2c + 0x1c,
            type_definition_namespace: 0x30 + 0x20,
            type_definition_v_table_size: 0x38 + 0x24,
            type_definition_size: 0x5c + 0x20 + 0x18 - 0x4,
            type_definition_fields: 0x60 + 0x20 + 0x18,
            type_definition_by_val_arg: 0x74 + 0x44,
            type_definition_runtime_info: 0x84 + 0x34 + 0x18,
            type_definition_field_count: 0xa4 + 0x34 + 0x10 + 0x18,
            type_definition_next_class_cache: 0xa8 + 0x34 + 0x10 + 0x18 + 0x4,
            type_definition_mono_generic_class: 0x94 + 0x34 + 0x18 + 0x10,
            type_definition_generic_container: 0x110,
            type_definition_runtime_info_domain_v_tables: 0x4 + 0x4,
            v_table: 0x28 + 0x18,
        }
    }
    
    /// Get the best matching offsets for a Unity version
    pub fn for_version(version: &UnityVersion) -> Self {
        if version.year >= 2022 {
            println!("Using Unity 2022.3 (experimental) offsets for version {}", version);
            Self::unity_2022_3()
        } else if version.year >= 2021 {
            println!("Using Unity 2021.3.14 offsets for version {}", version);
            Self::unity_2021_3_14()
        } else if version.year >= 2019 {
            println!("Using Unity 2019/2020 offsets for version {}", version);
            Self::unity_2019_2020()
        } else {
            println!("Unknown Unity version {}, defaulting to 2021.3.14 offsets", version);
            Self::unity_2021_3_14()
        }
    }
    
    /// Get all available offset profiles for probing
    pub fn all_profiles() -> Vec<Self> {
        vec![
            Self::unity_2022_3(),
            Self::unity_2021_3_14(),
            Self::unity_2019_2020(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_version() {
        let v = UnityVersion::parse("2021.3.14f1").unwrap();
        assert_eq!(v.year, 2021);
        assert_eq!(v.version_within_year, 3);
        assert_eq!(v.subversion_within_year, 14);
        
        let v2 = UnityVersion::parse("2019.4.5").unwrap();
        assert_eq!(v2.year, 2019);
        assert_eq!(v2.version_within_year, 4);
        assert_eq!(v2.subversion_within_year, 5);
    }
}
