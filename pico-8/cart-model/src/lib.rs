extern crate alloc;

use core::fmt;

use alloc::borrow::Cow;

use std::io;

pub mod header;
pub use header::Header;

pub mod section;
pub use section::{Section, SectionDelimiter, SectionType};

#[tracing::instrument(skip(cart_src), ret)]
pub fn get_section_delimiters(
    cart_src: &[u8],
    line_number_offset: Option<usize>,
) -> impl Iterator<Item = SectionDelimiter> {
    let mut byte_offset = 0;
    bytes::NewlineIter::new(cart_src)
        .enumerate()
        .filter_map(move |(line_number, line)| {
            let line_number_with_offset =
                line_number + (line_number_offset.unwrap_or_default() + 1);
            let delimiter = section::get_line_type(line).copied().map(|r#type| {
                tracing::debug!(
                    "Section of {type:?} starts at {line_number_with_offset}: {:?}",
                    core::str::from_utf8(line)
                );
                SectionDelimiter {
                    r#type,
                    line_number: line_number_with_offset,
                    byte_offset,
                }
            });
            if delimiter.is_none() {
                tracing::debug!(
                    "{line_number_with_offset}: {:?}",
                    core::str::from_utf8(line)
                );
            }
            byte_offset += line.len();
            delimiter
        })
}
#[tracing::instrument(level = "debug", skip(cart_src, delimiters), ret)]
pub fn get_sections(
    cart_src: &[u8],
    delimiters: impl Iterator<Item = SectionDelimiter>,
) -> impl Iterator<Item = Section<'_>> + '_ {
    // Collect so that we may sort
    let mut sorted_delimiters: Vec<SectionDelimiter> = delimiters.collect();

    // Sort so that we iterate in line with line-numbers
    sorted_delimiters.sort();

    let mut next_section_offset = 0;

    sorted_delimiters
        .into_iter()
        // Reverse, so we can traverse backwards and use an external
        // variable to track previous (next actually, since reverse)
        // section length
        .rev()
        .enumerate()
        .filter_map(
            move |(
                idx,
                SectionDelimiter {
                    r#type,
                    line_number,
                    byte_offset,
                },
            )| {
                let type_string =
                    <&'static str as From<&SectionType>>::from(&r#type);
                // Filter out the type-marker + `\n` before converting into section data
                //
                // We can always reverse the slice provided we need to recover it
                // (and still in borrowed cow-state)
                let offset_without_type_marker = byte_offset + type_string.len() + 1;
                let section_src = if idx == 0 {
                    cart_src.get(offset_without_type_marker..)
                } else {
                    cart_src.get(offset_without_type_marker..next_section_offset)
                }?;

                next_section_offset = byte_offset;

                let section = Section::new(r#type, line_number, section_src);

                tracing::debug!(
                    "[Line: {line_number:>4} | Size: {:>6} | Offset: {offset_without_type_marker:>6} -> {:>6}] {type:?}",
                    section_src.len(),
                    offset_without_type_marker + section_src.len()
                );
                Some(section)
            },
        )
}

const P8_MAX_CODE_EDITOR_TAB_COUNT: usize = 16;

/// Always of the `lua` type
struct Tab<'file> {
    line_number: usize,
    code_data: Cow<'file, [u8]>,
}

impl Tab<'_> {
    #[tracing::instrument(level = "debug", ret)]
    fn into_owned(self) -> Tab<'static> {
        let Tab {
            line_number,
            code_data,
        } = self;
        Tab {
            line_number,
            code_data: Cow::Owned(code_data.into_owned()),
        }
    }
}

impl fmt::Debug for Tab<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tab")
            .field("line_number", &self.line_number)
            .field("code_data.len()", &self.code_data.len())
            .finish_non_exhaustive()
    }
}

#[tracing::instrument(level = "debug", skip(section_data), ret)]
fn get_code_tabs_from_lua_section(
    mut line_number: usize,
    section_data: Cow<'_, [u8]>,
) -> [Option<Tab<'_>>; P8_MAX_CODE_EDITOR_TAB_COUNT] {
    let mut tabs: [Option<Tab<'_>>; P8_MAX_CODE_EDITOR_TAB_COUNT] = Default::default();

    // Increment over the __lua__ marker
    line_number += 1;

    let Cow::Borrowed(section_data) = section_data else {
        panic!("dont give me a vector eww");
    };

    for (tab_index, tab_data) in bytes::TabIter::from(section_data).enumerate() {
        let tab = Tab {
            line_number,
            code_data: Cow::Borrowed(section_data),
        };

        // Increment over previous iteration tab-separator (not for first)
        if tab_index != 0 {
            line_number += 1;
        };
        tabs[tab_index] = Some(tab);

        let lines_in_section = bytes::NewlineIter::new(tab_data).count();

        line_number += lines_in_section;
    }

    tabs
}

#[derive(Debug)]
struct Asset<'a> {
    line_number: usize,
    asset_data: Cow<'a, [u8]>,
}
impl Asset<'_> {
    #[tracing::instrument(level = "debug")]
    fn into_owned(self) -> Asset<'static> {
        let Asset {
            line_number,
            asset_data,
        } = self;
        Asset {
            line_number,
            asset_data: Cow::Owned(asset_data.into_owned()),
        }
    }
}

#[derive(Debug)]
struct Label<'a> {
    line_number: usize,
    label_data: Cow<'a, [u8]>,
}

impl Label<'_> {
    #[tracing::instrument(level = "debug")]
    fn into_owned(self) -> Label<'static> {
        let Label {
            line_number,
            label_data,
        } = self;
        Label {
            line_number,
            label_data: Cow::Owned(label_data.into_owned()),
        }
    }
}

#[derive(Debug)]
pub struct Cart<'a> {
    header: Cow<'a, Header>,
    /// The cart's label-data
    ///
    /// Optional field
    label: Option<Label<'a>>,
    /// All the lua-data in this cart
    code_tabs: [Option<Tab<'a>>; P8_MAX_CODE_EDITOR_TAB_COUNT],

    /// Always found in even empty pico-8 cartridge files
    ///
    /// All other fields are optional
    gfx: Asset<'a>,
    gff: Option<Asset<'a>>,
    map: Option<Asset<'a>>,
    sfx: Option<Asset<'a>>,
    music: Option<Asset<'a>>,
}
impl<'a> Cart<'a> {
    pub fn into_owned(self) -> Cart<'static> {
        let Cart {
            header,
            label,
            code_tabs,
            gfx,
            gff,
            map,
            sfx,
            music,
        } = self;
        let mut owned_tabs = <[Option<Tab<'_>>; P8_MAX_CODE_EDITOR_TAB_COUNT]>::default();
        for (tab_idx, tab_section) in code_tabs.into_iter().enumerate() {
            owned_tabs[tab_idx] = tab_section.map(Tab::into_owned);
        }
        Cart {
            header: Cow::Owned(header.into_owned()),
            label: label.map(Label::into_owned),
            code_tabs: owned_tabs,
            gfx: gfx.into_owned(),
            gff: gff.map(Asset::into_owned),
            map: map.map(Asset::into_owned),
            sfx: sfx.map(Asset::into_owned),
            music: music.map(Asset::into_owned),
        }
    }
    #[tracing::instrument(level = "debug", skip(cart_src), ret)]
    pub fn from_cart_source(cart_src: &'a [u8]) -> io::Result<Cart<'a>> {
        let (header, remainder) = header::split_from(cart_src).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Failed to parse header from cart-source of {} bytes",
                    cart_src.len()
                ),
            )
        })?;

        CartBuilder::from_iter(get_sections(
            remainder,
            get_section_delimiters(remainder, Some(2)),
        ))
        .build_with(header)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Gfx was missing when building cart (or something else idk really)",
            )
        })
    }
}

#[derive(Debug, Default)]
struct CartBuilder<'a> {
    /// Optional field
    label: Option<Label<'a>>,

    // Asset fields
    /// Always found in even empty pico-8 cartridge files
    ///
    /// All other asset-fields are optional
    gfx: Option<Asset<'a>>,
    /// Optional field
    gff: Option<Asset<'a>>,
    /// Optional field
    map: Option<Asset<'a>>,
    /// Optional field
    sfx: Option<Asset<'a>>,
    /// Optional field
    music: Option<Asset<'a>>,

    /// All the lua-data in this cart
    code_tabs: [Option<Tab<'a>>; P8_MAX_CODE_EDITOR_TAB_COUNT],
}

impl<'a> FromIterator<Section<'a>> for CartBuilder<'a> {
    #[tracing::instrument(level = "debug", skip(iter), ret)]
    fn from_iter<T: IntoIterator<Item = Section<'a>>>(iter: T) -> Self {
        iter.into_iter()
            .fold(Default::default(), |acc, section| match section {
                Section::Lua {
                    line_number,
                    section_data,
                } => CartBuilder {
                    code_tabs: get_code_tabs_from_lua_section(line_number, section_data),
                    ..acc
                },
                Section::Gfx {
                    line_number,
                    section_data,
                } => CartBuilder {
                    gfx: Some(Asset {
                        line_number,
                        asset_data: section_data,
                    }),
                    ..acc
                },
                Section::Gff {
                    line_number,
                    section_data,
                } => CartBuilder {
                    gff: Some(Asset {
                        line_number,
                        asset_data: section_data,
                    }),
                    ..acc
                },
                Section::Sfx {
                    line_number,
                    section_data,
                } => CartBuilder {
                    sfx: Some(Asset {
                        line_number,
                        asset_data: section_data,
                    }),
                    ..acc
                },
                Section::Map {
                    line_number,
                    section_data,
                } => CartBuilder {
                    map: Some(Asset {
                        line_number,
                        asset_data: section_data,
                    }),
                    ..acc
                },
                Section::Music {
                    line_number,
                    section_data,
                } => CartBuilder {
                    music: Some(Asset {
                        line_number,
                        asset_data: section_data,
                    }),
                    ..acc
                },
                Section::Label {
                    line_number,
                    section_data,
                } => CartBuilder {
                    label: Some(Label {
                        line_number,
                        label_data: section_data,
                    }),
                    ..acc
                },
            })
    }
}

impl<'a> CartBuilder<'a> {
    /// requires header to start
    #[tracing::instrument(level = "debug", ret)]
    fn build_with(self, header: &'a Header) -> Option<Cart<'a>> {
        let CartBuilder {
            label,
            gfx,
            gff,
            map,
            sfx,
            music,
            code_tabs,
        } = self;
        Some(Cart {
            header: Cow::Borrowed(header),
            label,

            gfx: gfx?,
            gff,
            map,
            sfx,
            music,

            code_tabs,
        })
    }
}
