use core::fmt;
use core::fmt::Formatter;
use std::fmt::Display;

pub enum TypeCode {
    END = 0x00,
    VOID = 0x01,
    BOOLEAN = 0x02,

    // [Description("char")]
    CHAR = 0x03,

    // [Description("byte")]
    I1 = 0x04,

    // [Description("sbyte")]
    U1 = 0x05,

    // [Description("short")]
    I2 = 0x06,

    // [Description("ushort")]
    U2 = 0x07,

    // [Description("int")]
    I4 = 0x08,

    // [Description("uint")]
    U4 = 0x09,

    // [Description("long")]
    I8 = 0x0a,

    // [Description("ulong")]
    U8 = 0x0b,

    // [Description("float")]
    R4 = 0x0c,

    // [Description("double")]
    R8 = 0x0d,

    // [Description("string")]
    STRING = 0x0e,

    PTR = 0x0f,         /* arg: <type> token */
    BYREF = 0x10,       /* arg: <type> token */
    VALUETYPE = 0x11,   /* arg: <type> token */
    CLASS = 0x12,       /* arg: <type> token */
    VAR = 0x13,         /* number */
    ARRAY = 0x14,       /* type, rank, boundsCount, bound1, loCount, lo1 */
    GENERICINST = 0x15, /* <type> <type-arg-count> <type-1> \x{2026} <type-n> */
    TYPEDBYREF = 0x16,

    // [Description("int")]
    I = 0x18,

    // [Description("uint")]
    U = 0x19,

    FNPTR = 0x1b, /* arg: full method signature */
    OBJECT = 0x1c,
    SZARRAY = 0x1d,  /* 0-based one-dim-array */
    MVAR = 0x1e,     /* number */
    CMODREQD = 0x1f, /* arg: typedef or typeref token */
    CMODOPT = 0x20,  /* optional arg: typedef or typref token */
    INTERNAL = 0x21, /* CLR internal type */
    MODIFIER = 0x40, /* Or with the following types */
    SENTINEL = 0x41, /* Sentinel for varargs method signature */
    PINNED = 0x45,   /* Local var that points to pinned object */
    ENUM = 0x55,     /* an enumeration */
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
