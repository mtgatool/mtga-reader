//! Type codes for .NET/Mono/IL2CPP type system
//! These codes are shared between Mono and IL2CPP runtimes

use core::fmt;
use core::fmt::Formatter;
use std::fmt::Display;

/// Represents a .NET type code used in both Mono and IL2CPP runtimes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeCode {
    END = 0x00,
    VOID = 0x01,
    BOOLEAN = 0x02,
    CHAR = 0x03,
    I1 = 0x04,      // byte
    U1 = 0x05,      // sbyte
    I2 = 0x06,      // short
    U2 = 0x07,      // ushort
    I4 = 0x08,      // int
    U4 = 0x09,      // uint
    I8 = 0x0a,      // long
    U8 = 0x0b,      // ulong
    R4 = 0x0c,      // float
    R8 = 0x0d,      // double
    STRING = 0x0e,
    PTR = 0x0f,
    BYREF = 0x10,
    VALUETYPE = 0x11,
    CLASS = 0x12,
    VAR = 0x13,
    ARRAY = 0x14,
    GENERICINST = 0x15,
    TYPEDBYREF = 0x16,
    I = 0x18,       // native int
    U = 0x19,       // native uint
    FNPTR = 0x1b,
    OBJECT = 0x1c,
    SZARRAY = 0x1d,
    MVAR = 0x1e,
    CMODREQD = 0x1f,
    CMODOPT = 0x20,
    INTERNAL = 0x21,
    MODIFIER = 0x40,
    SENTINEL = 0x41,
    PINNED = 0x45,
    ENUM = 0x55,
}

impl TypeCode {
    /// Convert a raw u32 type code value to TypeCode enum
    pub fn from_raw(value: u32) -> TypeCode {
        match value {
            0x00 => TypeCode::END,
            0x01 => TypeCode::VOID,
            0x02 => TypeCode::BOOLEAN,
            0x03 => TypeCode::CHAR,
            0x04 => TypeCode::I1,
            0x05 => TypeCode::U1,
            0x06 => TypeCode::I2,
            0x07 => TypeCode::U2,
            0x08 => TypeCode::I4,
            0x09 => TypeCode::U4,
            0x0a => TypeCode::I8,
            0x0b => TypeCode::U8,
            0x0c => TypeCode::R4,
            0x0d => TypeCode::R8,
            0x0e => TypeCode::STRING,
            0x0f => TypeCode::PTR,
            0x10 => TypeCode::BYREF,
            0x11 => TypeCode::VALUETYPE,
            0x12 => TypeCode::CLASS,
            0x13 => TypeCode::VAR,
            0x14 => TypeCode::ARRAY,
            0x15 => TypeCode::GENERICINST,
            0x16 => TypeCode::TYPEDBYREF,
            0x18 => TypeCode::I,
            0x19 => TypeCode::U,
            0x1b => TypeCode::FNPTR,
            0x1c => TypeCode::OBJECT,
            0x1d => TypeCode::SZARRAY,
            0x1e => TypeCode::MVAR,
            0x1f => TypeCode::CMODREQD,
            0x20 => TypeCode::CMODOPT,
            0x21 => TypeCode::INTERNAL,
            0x40 => TypeCode::MODIFIER,
            0x41 => TypeCode::SENTINEL,
            0x45 => TypeCode::PINNED,
            0x55 => TypeCode::ENUM,
            _ => TypeCode::END,
        }
    }

    /// Get the size in bytes for this type code
    pub fn size(&self) -> usize {
        match self {
            TypeCode::BOOLEAN | TypeCode::I1 | TypeCode::U1 => 1,
            TypeCode::CHAR | TypeCode::I2 | TypeCode::U2 => 2,
            TypeCode::I4 | TypeCode::U4 | TypeCode::R4 => 4,
            TypeCode::I8 | TypeCode::U8 | TypeCode::R8 => 8,
            // Pointer-sized types (assuming 64-bit)
            TypeCode::PTR | TypeCode::BYREF | TypeCode::CLASS | TypeCode::OBJECT |
            TypeCode::SZARRAY | TypeCode::ARRAY | TypeCode::GENERICINST |
            TypeCode::FNPTR | TypeCode::I | TypeCode::U => 8,
            // Value types need to be determined at runtime
            TypeCode::VALUETYPE => 4,
            _ => 0,
        }
    }

    /// Check if this type is a primitive type
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            TypeCode::BOOLEAN | TypeCode::CHAR |
            TypeCode::I1 | TypeCode::U1 | TypeCode::I2 | TypeCode::U2 |
            TypeCode::I4 | TypeCode::U4 | TypeCode::I8 | TypeCode::U8 |
            TypeCode::R4 | TypeCode::R8 | TypeCode::I | TypeCode::U
        )
    }

    /// Check if this type is a reference type (pointer to object)
    pub fn is_reference(&self) -> bool {
        matches!(
            self,
            TypeCode::CLASS | TypeCode::OBJECT | TypeCode::SZARRAY |
            TypeCode::ARRAY | TypeCode::STRING | TypeCode::GENERICINST
        )
    }
}

impl Display for TypeCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TypeCode::END => write!(f, "END"),
            TypeCode::VOID => write!(f, "VOID"),
            TypeCode::BOOLEAN => write!(f, "BOOLEAN"),
            TypeCode::CHAR => write!(f, "CHAR"),
            TypeCode::I1 => write!(f, "BYTE (I1)"),
            TypeCode::U1 => write!(f, "UBYTE (U1)"),
            TypeCode::I2 => write!(f, "SHORT (I2)"),
            TypeCode::U2 => write!(f, "USHORT (U2)"),
            TypeCode::I4 => write!(f, "INT (I4)"),
            TypeCode::U4 => write!(f, "UINT (U4)"),
            TypeCode::I8 => write!(f, "LONG (I8)"),
            TypeCode::U8 => write!(f, "ULONG (U8)"),
            TypeCode::R4 => write!(f, "FLOAT (R4)"),
            TypeCode::R8 => write!(f, "DOUBLE (R8)"),
            TypeCode::STRING => write!(f, "STRING"),
            TypeCode::PTR => write!(f, "PTR"),
            TypeCode::BYREF => write!(f, "BYREF"),
            TypeCode::VALUETYPE => write!(f, "VALUETYPE"),
            TypeCode::CLASS => write!(f, "CLASS"),
            TypeCode::VAR => write!(f, "VAR"),
            TypeCode::ARRAY => write!(f, "ARRAY"),
            TypeCode::GENERICINST => write!(f, "GENERICINST"),
            TypeCode::TYPEDBYREF => write!(f, "TYPEDBYREF"),
            TypeCode::I => write!(f, "INT (I)"),
            TypeCode::U => write!(f, "UINT (U)"),
            TypeCode::FNPTR => write!(f, "FNPTR"),
            TypeCode::OBJECT => write!(f, "OBJECT"),
            TypeCode::SZARRAY => write!(f, "SZARRAY"),
            TypeCode::MVAR => write!(f, "MVAR"),
            TypeCode::CMODREQD => write!(f, "CMOD_REQD"),
            TypeCode::CMODOPT => write!(f, "CMOD_OPT"),
            TypeCode::INTERNAL => write!(f, "INTERNAL"),
            TypeCode::MODIFIER => write!(f, "MODIFIER"),
            TypeCode::SENTINEL => write!(f, "SENTINEL"),
            TypeCode::PINNED => write!(f, "PINNED"),
            TypeCode::ENUM => write!(f, "ENUM"),
        }
    }
}
