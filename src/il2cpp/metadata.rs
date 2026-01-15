//! IL2CPP metadata parser
//!
//! Parses the global-metadata.dat file that contains IL2CPP type information.
//! Supports metadata versions 24-31.

use std::path::Path;
use std::io::{self, Read};
use std::fs::File;

/// Magic number at the start of global-metadata.dat
const METADATA_SANITY: u32 = 0xFAB11BAF;

/// Header indices for v31 metadata (offset/size pairs starting at byte 8)
mod v31_indices {
    pub const STRINGS: usize = 2;
    pub const FIELDS: usize = 8;
    pub const TYPE_DEFINITIONS: usize = 19;
    pub const IMAGES: usize = 20;
}

/// Struct sizes for v31 metadata
mod v31_sizes {
    pub const TYPE_DEFINITION: usize = 88;
    pub const IMAGE_DEFINITION: usize = 40;
    pub const FIELD_DEFINITION: usize = 12;
}

/// Header offsets extracted from metadata
#[derive(Debug, Clone)]
struct HeaderOffsets {
    strings_offset: usize,
    strings_size: usize,
    fields_offset: usize,
    fields_size: usize,
    type_definitions_offset: usize,
    type_definitions_size: usize,
    images_offset: usize,
    images_size: usize,
}

/// IL2CPP type definition from metadata (v31 layout, 88 bytes)
#[derive(Debug, Clone)]
pub struct Il2CppTypeDefinition {
    pub name_index: i32,
    pub namespace_index: i32,
    pub byval_type_index: i32,
    pub declaring_type_index: i32,
    pub parent_index: i32,
    pub element_type_index: i32,
    pub generic_container_index: i32,
    pub flags: u32,
    pub field_start: i32,
    pub method_start: i32,
    pub event_start: i32,
    pub property_start: i32,
    pub nested_types_start: i32,
    pub interfaces_start: i32,
    pub vtable_start: i32,
    pub interface_offsets_start: i32,
    pub method_count: u16,
    pub property_count: u16,
    pub field_count: u16,
    pub event_count: u16,
    pub nested_types_count: u16,
    pub vtable_count: u16,
    pub interfaces_count: u16,
    pub interface_offsets_count: u16,
    pub bit_field: u32,
    pub token: u32,
}

impl Il2CppTypeDefinition {
    /// Parse a type definition from bytes (v31 format, 88 bytes)
    fn from_bytes(data: &[u8], offset: usize) -> Option<Self> {
        if offset + v31_sizes::TYPE_DEFINITION > data.len() {
            return None;
        }

        Some(Il2CppTypeDefinition {
            name_index: read_i32(data, offset),
            namespace_index: read_i32(data, offset + 4),
            byval_type_index: read_i32(data, offset + 8),
            declaring_type_index: read_i32(data, offset + 12),
            parent_index: read_i32(data, offset + 16),
            element_type_index: read_i32(data, offset + 20),
            generic_container_index: read_i32(data, offset + 24),
            flags: read_u32(data, offset + 28),
            field_start: read_i32(data, offset + 32),
            method_start: read_i32(data, offset + 36),
            event_start: read_i32(data, offset + 40),
            property_start: read_i32(data, offset + 44),
            nested_types_start: read_i32(data, offset + 48),
            interfaces_start: read_i32(data, offset + 52),
            vtable_start: read_i32(data, offset + 56),
            interface_offsets_start: read_i32(data, offset + 60),
            method_count: read_u16(data, offset + 72),
            property_count: read_u16(data, offset + 74),
            field_count: read_u16(data, offset + 76),
            event_count: read_u16(data, offset + 78),
            nested_types_count: read_u16(data, offset + 80),
            vtable_count: read_u16(data, offset + 82),
            interfaces_count: read_u16(data, offset + 84),
            interface_offsets_count: read_u16(data, offset + 86),
            bit_field: read_u32(data, offset + 64),
            token: read_u32(data, offset + 68),
        })
    }
}

/// IL2CPP field definition from metadata (12 bytes)
#[derive(Debug, Clone)]
pub struct Il2CppFieldDefinition {
    pub name_index: i32,
    pub type_index: i32,
    pub token: u32,
}

impl Il2CppFieldDefinition {
    fn from_bytes(data: &[u8], offset: usize) -> Option<Self> {
        if offset + v31_sizes::FIELD_DEFINITION > data.len() {
            return None;
        }

        Some(Il2CppFieldDefinition {
            name_index: read_i32(data, offset),
            type_index: read_i32(data, offset + 4),
            token: read_u32(data, offset + 8),
        })
    }
}

/// IL2CPP image definition from metadata (v31 layout, 40 bytes)
#[derive(Debug, Clone)]
pub struct Il2CppImageDefinition {
    pub name_index: i32,
    pub assembly_index: i32,
    pub type_start: i32,
    pub type_count: u32,
    pub exported_type_start: i32,
    pub exported_type_count: u32,
    pub entry_point_index: i32,
    pub token: u32,
    pub custom_attribute_start: i32,
    pub custom_attribute_count: u32,
}

impl Il2CppImageDefinition {
    fn from_bytes(data: &[u8], offset: usize) -> Option<Self> {
        if offset + v31_sizes::IMAGE_DEFINITION > data.len() {
            return None;
        }

        Some(Il2CppImageDefinition {
            name_index: read_i32(data, offset),
            assembly_index: read_i32(data, offset + 4),
            type_start: read_i32(data, offset + 8),
            type_count: read_u32(data, offset + 12),
            exported_type_start: read_i32(data, offset + 16),
            exported_type_count: read_u32(data, offset + 20),
            entry_point_index: read_i32(data, offset + 24),
            token: read_u32(data, offset + 28),
            custom_attribute_start: read_i32(data, offset + 32),
            custom_attribute_count: read_u32(data, offset + 36),
        })
    }
}

// Helper functions for reading binary data
fn read_i32(data: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

/// Error type for metadata parsing
#[derive(Debug)]
pub enum MetadataError {
    IoError(io::Error),
    InvalidMagic,
    UnsupportedVersion(i32),
    InvalidOffset,
}

impl From<io::Error> for MetadataError {
    fn from(e: io::Error) -> Self {
        MetadataError::IoError(e)
    }
}

impl std::fmt::Display for MetadataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetadataError::IoError(e) => write!(f, "IO error: {}", e),
            MetadataError::InvalidMagic => write!(f, "Invalid metadata magic number"),
            MetadataError::UnsupportedVersion(v) => write!(f, "Unsupported metadata version: {}", v),
            MetadataError::InvalidOffset => write!(f, "Invalid offset in metadata"),
        }
    }
}

impl std::error::Error for MetadataError {}

/// Parser for IL2CPP global-metadata.dat
#[derive(Debug)]
pub struct MetadataParser {
    data: Vec<u8>,
    version: i32,
    offsets: HeaderOffsets,
}

impl MetadataParser {
    /// Parse metadata from a file
    pub fn from_file(path: &Path) -> Result<Self, MetadataError> {
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        Self::from_bytes(data)
    }

    /// Parse metadata from bytes
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, MetadataError> {
        if data.len() < 256 {
            return Err(MetadataError::InvalidOffset);
        }

        // Validate magic
        let magic = read_u32(&data, 0);
        if magic != METADATA_SANITY {
            return Err(MetadataError::InvalidMagic);
        }

        let version = read_i32(&data, 4);

        // Check version (supported: 24-31)
        if version < 24 {
            return Err(MetadataError::UnsupportedVersion(version));
        }

        // For v31 metadata, header is array of offset/size pairs starting at byte 8
        // Each pair is (i32 offset, i32 size) = 8 bytes
        let offsets = if version >= 29 {
            // v29+ uses indexed offset/size pairs
            HeaderOffsets {
                strings_offset: read_i32(&data, 8 + v31_indices::STRINGS * 8) as usize,
                strings_size: read_i32(&data, 8 + v31_indices::STRINGS * 8 + 4) as usize,
                fields_offset: read_i32(&data, 8 + v31_indices::FIELDS * 8) as usize,
                fields_size: read_i32(&data, 8 + v31_indices::FIELDS * 8 + 4) as usize,
                type_definitions_offset: read_i32(&data, 8 + v31_indices::TYPE_DEFINITIONS * 8) as usize,
                type_definitions_size: read_i32(&data, 8 + v31_indices::TYPE_DEFINITIONS * 8 + 4) as usize,
                images_offset: read_i32(&data, 8 + v31_indices::IMAGES * 8) as usize,
                images_size: read_i32(&data, 8 + v31_indices::IMAGES * 8 + 4) as usize,
            }
        } else {
            // Older versions use the traditional header layout
            // String offset is at byte 24 (index 6 in older format)
            HeaderOffsets {
                strings_offset: read_i32(&data, 24) as usize,
                strings_size: read_i32(&data, 28) as usize,
                fields_offset: read_i32(&data, 64) as usize,
                fields_size: read_i32(&data, 68) as usize,
                type_definitions_offset: read_i32(&data, 160) as usize,
                type_definitions_size: read_i32(&data, 164) as usize,
                images_offset: read_i32(&data, 168) as usize,
                images_size: read_i32(&data, 172) as usize,
            }
        };

        Ok(MetadataParser { data, version, offsets })
    }

    /// Get metadata version
    pub fn version(&self) -> i32 {
        self.version
    }

    /// Get a string from the string table
    pub fn get_string(&self, index: i32) -> Option<&str> {
        if index < 0 {
            return None;
        }

        let offset = self.offsets.strings_offset + index as usize;
        if offset >= self.data.len() {
            return None;
        }

        // Find null terminator
        let end = self.data[offset..].iter().position(|&b| b == 0)?;
        std::str::from_utf8(&self.data[offset..offset + end]).ok()
    }

    /// Get number of type definitions
    pub fn type_definition_count(&self) -> usize {
        self.offsets.type_definitions_size / v31_sizes::TYPE_DEFINITION
    }

    /// Get a type definition by index
    pub fn get_type_definition(&self, index: usize) -> Option<Il2CppTypeDefinition> {
        let offset = self.offsets.type_definitions_offset + index * v31_sizes::TYPE_DEFINITION;
        Il2CppTypeDefinition::from_bytes(&self.data, offset)
    }

    /// Get number of field definitions
    pub fn field_definition_count(&self) -> usize {
        self.offsets.fields_size / v31_sizes::FIELD_DEFINITION
    }

    /// Get a field definition by index
    pub fn get_field_definition(&self, index: usize) -> Option<Il2CppFieldDefinition> {
        let offset = self.offsets.fields_offset + index * v31_sizes::FIELD_DEFINITION;
        Il2CppFieldDefinition::from_bytes(&self.data, offset)
    }

    /// Get number of images (assemblies)
    pub fn image_count(&self) -> usize {
        self.offsets.images_size / v31_sizes::IMAGE_DEFINITION
    }

    /// Get an image definition by index
    pub fn get_image_definition(&self, index: usize) -> Option<Il2CppImageDefinition> {
        let offset = self.offsets.images_offset + index * v31_sizes::IMAGE_DEFINITION;
        Il2CppImageDefinition::from_bytes(&self.data, offset)
    }

    /// Get all assembly/image names
    pub fn get_assembly_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        let count = self.image_count();

        for i in 0..count {
            if let Some(image) = self.get_image_definition(i) {
                if let Some(name) = self.get_string(image.name_index) {
                    names.push(name.to_string());
                }
            }
        }

        names
    }

    /// Find type definitions in a specific image (assembly)
    pub fn get_types_in_image(&self, image_index: usize) -> Vec<(usize, Il2CppTypeDefinition)> {
        let mut types = Vec::new();

        if let Some(image) = self.get_image_definition(image_index) {
            let start = image.type_start as usize;
            let count = image.type_count as usize;

            for i in 0..count {
                if let Some(type_def) = self.get_type_definition(start + i) {
                    types.push((start + i, type_def));
                }
            }
        }

        types
    }

    /// Find an image by name
    pub fn find_image(&self, name: &str) -> Option<(usize, Il2CppImageDefinition)> {
        for i in 0..self.image_count() {
            if let Some(image) = self.get_image_definition(i) {
                if let Some(img_name) = self.get_string(image.name_index) {
                    if img_name.contains(name) {
                        return Some((i, image));
                    }
                }
            }
        }
        None
    }

    /// Find a type by name
    pub fn find_type(&self, name: &str) -> Option<(usize, Il2CppTypeDefinition)> {
        for i in 0..self.type_definition_count() {
            if let Some(type_def) = self.get_type_definition(i) {
                if let Some(type_name) = self.get_string(type_def.name_index) {
                    if type_name == name {
                        return Some((i, type_def));
                    }
                }
            }
        }
        None
    }

    /// Find a type by name and namespace
    pub fn find_type_in_namespace(&self, namespace: &str, name: &str) -> Option<(usize, Il2CppTypeDefinition)> {
        for i in 0..self.type_definition_count() {
            if let Some(type_def) = self.get_type_definition(i) {
                if let Some(type_name) = self.get_string(type_def.name_index) {
                    if type_name == name {
                        let type_ns = self.get_string(type_def.namespace_index).unwrap_or("");
                        if type_ns == namespace {
                            return Some((i, type_def));
                        }
                    }
                }
            }
        }
        None
    }
}
