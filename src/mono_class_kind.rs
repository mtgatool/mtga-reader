use std::fmt;
use std::fmt::Formatter;
use std::fmt::Debug;

#[derive(Clone)]
pub enum MonoClassKind {
    Def = 1,
    GTg = 2,
    GInst = 3,
    GParam = 4,
    Array = 5,
    Pointer = 6,
}

impl Debug for MonoClassKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            MonoClassKind::Def => write!(f, "Def"),
            MonoClassKind::GTg => write!(f, "GTg"),
            MonoClassKind::GInst => write!(f, "GInst"),
            MonoClassKind::GParam => write!(f, "GParam"),
            MonoClassKind::Array => write!(f, "Array"),
            MonoClassKind::Pointer => write!(f, "Pointer"),
        }
    }
}

pub fn match_class_kind(value: u8) -> MonoClassKind {
    match value {
        1 => MonoClassKind::Def,
        2 => MonoClassKind::GTg,
        3 => MonoClassKind::GInst,
        4 => MonoClassKind::GParam,
        5 => MonoClassKind::Array,
        6 => MonoClassKind::Pointer,
        _ => MonoClassKind::Def,
    }
}