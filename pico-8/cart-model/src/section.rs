use alloc::borrow::Cow;
use core::fmt;

/// A section in a .p8 cartridge file
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionType {
    Lua,
    Gfx,
    Gff,
    Label,
    Map,
    Sfx,
    Music,
}

impl From<SectionType> for &'static str {
    fn from(value: SectionType) -> Self {
        <&'static str as From<&SectionType>>::from(&value)
    }
}

impl From<&SectionType> for &'static str {
    fn from(value: &SectionType) -> Self {
        match value {
            SectionType::Lua => "__lua__",
            SectionType::Gfx => "__gfx__",
            SectionType::Gff => "__gff__",
            SectionType::Label => "__label__",
            SectionType::Map => "__map__",
            SectionType::Sfx => "__sfx__",
            SectionType::Music => "__music__",
        }
    }
}

const SECTION_TYPES: &[SectionType] = &[
    SectionType::Lua,
    SectionType::Gfx,
    SectionType::Gff,
    SectionType::Label,
    SectionType::Map,
    SectionType::Sfx,
    SectionType::Music,
];

pub fn get_line_type<T: AsRef<[u8]> + ?Sized>(line_src: &T) -> Option<&'static SectionType> {
    SECTION_TYPES.iter().find(|ty| {
        let needle = <&'static str as From<&SectionType>>::from(*ty).as_bytes();
        line_src.as_ref().starts_with(needle)
    })
}

#[derive(Debug, PartialEq, Eq)]
pub struct SectionDelimiter {
    /// The type of section
    pub r#type: SectionType,
    /// The (0-based) index of the line when first discovering this section
    pub line_number: usize,
    /// The reader-offset when first discovering this section
    pub byte_offset: usize,
}

#[allow(clippy::non_canonical_partial_ord_impl)] // false positives on some toolchains
impl PartialOrd for SectionDelimiter {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.line_number.cmp(&other.line_number))
    }
}

impl Ord for SectionDelimiter {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.line_number.cmp(&other.line_number)
    }
}

#[derive(PartialEq, Eq)]
pub enum Section<'a> {
    Lua {
        line_number: usize,
        section_data: Cow<'a, [u8]>,
    },
    Gfx {
        line_number: usize,
        section_data: Cow<'a, [u8]>,
    },
    Gff {
        line_number: usize,
        section_data: Cow<'a, [u8]>,
    },
    Label {
        line_number: usize,
        section_data: Cow<'a, [u8]>,
    },
    Map {
        line_number: usize,
        section_data: Cow<'a, [u8]>,
    },
    Sfx {
        line_number: usize,
        section_data: Cow<'a, [u8]>,
    },
    Music {
        line_number: usize,
        section_data: Cow<'a, [u8]>,
    },
}

#[allow(clippy::non_canonical_partial_ord_impl)] // false positives on some toolchains
impl PartialOrd for Section<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.line_number().cmp(&other.line_number()))
    }
}

impl Ord for Section<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.line_number().cmp(&other.line_number())
    }
}

impl<'a> Section<'a> {
    fn new_inner(
        r#type: SectionType,
        line_number: usize,
        section_data: Cow<'_, [u8]>,
    ) -> Section<'_> {
        match r#type {
            SectionType::Lua => Section::Lua {
                line_number,
                section_data,
            },
            SectionType::Gfx => Section::Gfx {
                line_number,
                section_data,
            },
            SectionType::Gff => Section::Gff {
                line_number,
                section_data,
            },
            SectionType::Label => Section::Label {
                line_number,
                section_data,
            },
            SectionType::Map => Section::Map {
                line_number,
                section_data,
            },
            SectionType::Sfx => Section::Sfx {
                line_number,
                section_data,
            },
            SectionType::Music => Section::Music {
                line_number,
                section_data,
            },
        }
    }
    pub fn new<T: AsRef<[u8]> + ?Sized>(
        r#type: SectionType,
        line_number: usize,
        data: &'a T,
    ) -> Section<'a> {
        let bytes: &'a [u8] = data.as_ref();
        let data: Cow<'a, [u8]> = Cow::Borrowed(bytes);
        Section::new_inner(r#type, line_number, data)
    }
    pub fn line_number(&self) -> usize {
        match self {
            Section::Lua { line_number, .. }
            | Section::Gfx { line_number, .. }
            | Section::Gff { line_number, .. }
            | Section::Label { line_number, .. }
            | Section::Map { line_number, .. }
            | Section::Sfx { line_number, .. }
            | Section::Music { line_number, .. } => *line_number,
        }
    }
    pub fn data(&self) -> &Cow<'a, [u8]> {
        match self {
            Section::Lua { section_data, .. }
            | Section::Gfx { section_data, .. }
            | Section::Gff { section_data, .. }
            | Section::Label { section_data, .. }
            | Section::Map { section_data, .. }
            | Section::Sfx { section_data, .. }
            | Section::Music { section_data, .. } => section_data,
        }
    }
    pub const fn get_type(&self) -> SectionType {
        match self {
            Section::Lua { .. } => SectionType::Lua,
            Section::Gfx { .. } => SectionType::Gfx,
            Section::Gff { .. } => SectionType::Gff,
            Section::Label { .. } => SectionType::Label,
            Section::Map { .. } => SectionType::Map,
            Section::Sfx { .. } => SectionType::Sfx,
            Section::Music { .. } => SectionType::Music,
        }
    }
    fn unwrap(self) -> (SectionType, usize, Cow<'a, [u8]>) {
        let r#type = self.get_type();
        match self {
            Section::Lua {
                section_data,
                line_number,
            }
            | Section::Gfx {
                section_data,
                line_number,
            }
            | Section::Gff {
                section_data,
                line_number,
            }
            | Section::Label {
                section_data,
                line_number,
            }
            | Section::Map {
                section_data,
                line_number,
            }
            | Section::Sfx {
                section_data,
                line_number,
            }
            | Section::Music {
                section_data,
                line_number,
            } => (r#type, line_number, section_data),
        }
    }
    pub fn into_owned(self) -> Section<'static> {
        let (r#type, line_number, data) = self.unwrap();
        let data = Cow::Owned(data.into_owned());
        Section::new_inner(r#type, line_number, data)
    }
}
impl fmt::Debug for Section<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Section")
            .field("type", &self.get_type())
            .field("line_number", &self.line_number())
            .field("data.len()", &self.data().len())
            .finish_non_exhaustive()
    }
}
