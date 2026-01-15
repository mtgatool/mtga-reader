//! Class kind enumeration shared between Mono and IL2CPP

/// Represents the kind of a class/type definition
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassKind {
    /// Regular class definition
    Def = 1,
    /// Generic type definition
    GenericTypeDef = 2,
    /// Generic instance
    GenericInst = 3,
    /// Generic parameter
    GenericParam = 4,
    /// Array type
    Array = 5,
    /// Pointer type
    Pointer = 6,
    /// Unknown class kind
    Unknown = 0,
}

impl ClassKind {
    /// Convert a raw byte value to ClassKind
    pub fn from_raw(value: u8) -> ClassKind {
        match value {
            1 => ClassKind::Def,
            2 => ClassKind::GenericTypeDef,
            3 => ClassKind::GenericInst,
            4 => ClassKind::GenericParam,
            5 => ClassKind::Array,
            6 => ClassKind::Pointer,
            _ => ClassKind::Unknown,
        }
    }
}

impl std::fmt::Display for ClassKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClassKind::Def => write!(f, "Def"),
            ClassKind::GenericTypeDef => write!(f, "GenericTypeDef"),
            ClassKind::GenericInst => write!(f, "GenericInst"),
            ClassKind::GenericParam => write!(f, "GenericParam"),
            ClassKind::Array => write!(f, "Array"),
            ClassKind::Pointer => write!(f, "Pointer"),
            ClassKind::Unknown => write!(f, "Unknown"),
        }
    }
}
