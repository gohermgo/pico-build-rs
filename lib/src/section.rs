use core::borrow::Borrow;
use core::fmt;
use core::mem::transmute;
use core::ops::Deref;

use alloc::borrow::Cow;

/// A section in a .p8 cartridge file
#[derive(Debug, PartialEq, Eq)]
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
pub struct SectionDelimiter<'a> {
    /// The type of section
    pub(crate) ty: &'a SectionType,
    /// The (0-based) index of the line when first discovering this section
    pub(crate) line_number: usize,
    /// The reader-offset when first discovering this section
    pub(crate) offset: usize,
}

#[allow(clippy::non_canonical_partial_ord_impl)]
impl PartialOrd for SectionDelimiter<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.line_number.cmp(&other.line_number))
    }
}

impl Ord for SectionDelimiter<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.line_number.cmp(&other.line_number)
    }
}

// struct Tab {
//     title: String,
//     code: Box<[u8]>,
// }

#[derive(Debug)]
pub struct SectionData(pub(crate) [u8]);

impl ToOwned for SectionData {
    type Owned = SectionDataBuf;
    fn to_owned(&self) -> Self::Owned {
        SectionDataBuf(Box::from(&self.0))
    }
}

impl SectionData {
    #[tracing::instrument(level = "debug", skip(self, seq))]
    pub fn split_at_sequence_exclusive<'a>(
        &'a self,
        seq: &[u8],
    ) -> Option<(&'a SectionData, &'a SectionData)> {
        bytes::split_at_sequence_exclusive(&self.0, seq).map(|pair| {
            let pair: (&'a SectionData, &'a SectionData) = unsafe { transmute(pair) };
            pair
        })
    }
}

#[derive(Debug)]
pub struct SectionDataBuf(Box<[u8]>);

impl Deref for SectionDataBuf {
    type Target = SectionData;
    fn deref(&self) -> &Self::Target {
        let slice = self.0.as_ref();
        let section_data: &SectionData = unsafe { transmute(slice) };
        section_data
    }
}

impl Borrow<SectionData> for SectionDataBuf {
    fn borrow(&self) -> &SectionData {
        self
    }
}

pub struct Section<'a> {
    pub(crate) ty: &'static SectionType,
    pub(crate) line_number: usize,
    pub(crate) data: Cow<'a, SectionData>,
}

impl Section<'_> {
    pub fn into_owned(self) -> Section<'static> {
        let Section {
            ty,
            line_number,
            data,
        } = self;
        Section {
            ty,
            line_number,
            data: Cow::Owned(data.into_owned()),
        }
    }
}

impl fmt::Debug for Section<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Section")
            .field("ty", &self.ty)
            .field("line_number", &self.line_number)
            .field("data.len()", &self.data.0.len())
            .finish_non_exhaustive()
    }
}
