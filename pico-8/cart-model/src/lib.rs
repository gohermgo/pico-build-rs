#![feature(debug_closure_helpers)]

extern crate alloc;

use core::fmt;

use alloc::borrow::Cow;

use std::io;

pub mod header;
pub use header::Header;

pub mod section;
pub use section::{Section, SectionDelimiter, SectionType};

#[tracing::instrument(skip(cart_src))]
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
                tracing::debug!("{type:?}-Section starts at {line_number_with_offset}",);
                SectionDelimiter {
                    r#type,
                    line_number: line_number_with_offset,
                    byte_offset,
                }
            });
            if delimiter.is_none() {
                tracing::trace!(
                    "{line_number_with_offset}: {:?}",
                    core::str::from_utf8(line)
                );
            }
            byte_offset += line.len();
            delimiter
        })
}
#[tracing::instrument(level = "debug", skip(cart_src, delimiters))]
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
                let type_string = format!("{type:?}");
                tracing::debug!(
                    "{type_string:<6} | Line: {line_number:>4} | Size: {:>6} | Offset: {offset_without_type_marker:>6} -> {:>6}",
                    section_src.len(),
                    offset_without_type_marker + section_src.len()
                );
                Some(section)
            },
        )
}

fn debug_section_type<'db, 'a, 'b>(
    mut f: &'db mut fmt::DebugStruct<'a, 'b>,
    r#type: Option<SectionType>,
    line_number: usize,
    data: &[u8],
    data_label: &'static str,
) -> &'db mut fmt::DebugStruct<'a, 'b>
where
    'b: 'a,
{
    if let Some(r#type) = r#type {
        f = f.field("type", &&r#type);
    }
    f.field("line_number", &&line_number)
        .field(format!("{data_label}.len()").as_str(), &&data.len())
}

const P8_MAX_CODE_EDITOR_TAB_COUNT: usize = 16;

/// Always of the `lua` type
pub struct Tab<'file> {
    pub line_number: usize,
    pub code_data: Cow<'file, [u8]>,
}

impl Tab<'_> {
    #[tracing::instrument(level = "debug", ret)]
    pub fn into_owned(self) -> Tab<'static> {
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
        debug_section_type(
            &mut f.debug_struct("Tab"),
            None,
            self.line_number,
            self.code_data.as_ref(),
            "code_data",
        )
        .finish_non_exhaustive()
    }
}

pub type CodeTabs<'a> = [Option<Tab<'a>>; P8_MAX_CODE_EDITOR_TAB_COUNT];

#[tracing::instrument(level = "debug", skip(section_data))]
pub fn get_code_tabs_from_lua_section<T: AsRef<[u8]> + ?Sized>(
    mut line_number: usize,
    section_data: &T,
) -> CodeTabs<'_> {
    let mut tabs: CodeTabs<'_> = Default::default();

    // Increment over the __lua__ marker
    line_number += 1;

    for (tab_index, tab_data) in bytes::TabIter::from(section_data).enumerate() {
        tracing::debug!("Tab {tab_index} of lua-code starts at {line_number}");
        let tab = Tab {
            line_number,
            code_data: Cow::Borrowed(tab_data),
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
    #[tracing::instrument(level = "debug", skip(self))]
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

struct Label<'a> {
    line_number: usize,
    label_data: Cow<'a, [u8]>,
}

impl Label<'_> {
    #[tracing::instrument(level = "debug", skip(self))]
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

impl fmt::Debug for Label<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        debug_section_type(
            &mut f.debug_struct("Label"),
            Some(SectionType::Label),
            self.line_number,
            self.label_data.as_ref(),
            "label_data",
        )
        .finish_non_exhaustive()
    }
}

pub struct CartData<'a> {
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

impl<'a> CartData<'a> {
    #[tracing::instrument(level = "debug", skip(gfx_data), ret)]
    pub fn from_parts(
        header: &'a Header,
        code_tabs: [Option<Tab<'a>>; P8_MAX_CODE_EDITOR_TAB_COUNT],
        gfx_data: &'a [u8],
    ) -> CartData<'a> {
        let lines_in_header: usize = bytes::NewlineIter::from(header).count();
        let lines_in_code: usize = code_tabs
            .iter()
            .filter_map(Option::as_ref)
            .map(|Tab { code_data, .. }| bytes::NewlineIter::from(code_data.as_ref()).count())
            .sum();
        let gfx_line_number = lines_in_header + lines_in_code;
        let gfx = Asset {
            line_number: gfx_line_number,
            asset_data: Cow::Borrowed(gfx_data),
        };
        CartData {
            header: Cow::Borrowed(header),
            gfx,
            label: None,
            code_tabs,
            gff: Default::default(),
            map: None,
            sfx: None,
            music: None,
        }
    }
}

/// Huge manual debug implementation to avoid spamming the terminal
impl fmt::Debug for CartData<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let CartData {
            header,
            label,
            code_tabs,
            gfx,
            gff,
            map,
            sfx,
            music,
        } = self;

        f.debug_struct("Cart")
            .field_with("header", |f| header.fmt(f))
            .field_with("label", |f| label.fmt(f))
            .field_with("code_tabs", |f| code_tabs.fmt(f))
            .field_with("gfx", |f| {
                debug_section_type(
                    &mut f.debug_struct("Asset"),
                    Some(SectionType::Gfx),
                    gfx.line_number,
                    gfx.asset_data.as_ref(),
                    "asset_data",
                )
                .finish_non_exhaustive()
            })
            .field_with("gff", |f| {
                if let Some(gff) = gff.as_ref() {
                    debug_section_type(
                        &mut f.debug_struct("Asset"),
                        Some(SectionType::Gff),
                        gff.line_number,
                        gff.asset_data.as_ref(),
                        "asset_data",
                    )
                    .finish_non_exhaustive()
                } else {
                    gff.fmt(f)
                }
            })
            .field_with("map", |f| {
                if let Some(map) = map.as_ref() {
                    debug_section_type(
                        &mut f.debug_struct("Asset"),
                        Some(SectionType::Map),
                        map.line_number,
                        map.asset_data.as_ref(),
                        "asset_data",
                    )
                    .finish_non_exhaustive()
                } else {
                    map.fmt(f)
                }
            })
            .field_with("sfx", |f| {
                if let Some(sfx) = sfx.as_ref() {
                    debug_section_type(
                        &mut f.debug_struct("Asset"),
                        Some(SectionType::Sfx),
                        sfx.line_number,
                        sfx.asset_data.as_ref(),
                        "asset_data",
                    )
                    .finish_non_exhaustive()
                } else {
                    sfx.fmt(f)
                }
            })
            .field_with("music", |f| {
                if let Some(music) = music.as_ref() {
                    debug_section_type(
                        &mut f.debug_struct("Asset"),
                        Some(SectionType::Music),
                        music.line_number,
                        music.asset_data.as_ref(),
                        "asset_data",
                    )
                    .finish_non_exhaustive()
                } else {
                    music.fmt(f)
                }
            })
            .finish()
    }
}
impl<'a> CartData<'a> {
    pub fn into_owned(self) -> CartData<'static> {
        let CartData {
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
        CartData {
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
    #[tracing::instrument(level = "trace", skip(cart_src))]
    pub fn from_cart_source(cart_src: &'a [u8]) -> io::Result<CartData<'a>> {
        tracing::debug!("FROM SOURCE");
        let (header, remainder) = header::split_from(cart_src).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Failed to parse header from cart-source of {} bytes",
                    cart_src.len()
                ),
            )
        })?;

        CartDataBuilder::from_iter(get_sections(
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
    /// Caution, will overwrite entirely
    #[tracing::instrument(level = "debug")]
    pub fn set_code_data(&mut self, code_tabs: CodeTabs<'a>) {
        self.code_tabs = code_tabs;
    }
    pub fn into_cart_source<T: FromIterator<u8>>(self) -> T {
        let CartData {
            header,
            label,
            code_tabs,
            gfx,
            gff,
            map,
            sfx,
            music,
        } = self;
        // 1. Header
        let header = header.into_owned().copy_to_boxed_slice();
        let iter = header.into_iter();
        // 2. Lua
        let mut code_tabs: Box<[u8]> = code_tabs
            .into_iter()
            .enumerate()
            .flat_map(|(idx, elt)| {
                elt.map(|Tab { code_data, .. }| {
                    if matches!(idx, 0) {
                        code_data.into_owned()
                    } else {
                        bytes::TAB_SEQUENCE
                            .iter()
                            .copied()
                            .chain(core::iter::once(b'\n'))
                            .chain(code_data.into_owned())
                            .collect()
                    }
                })
                .map(Vec::into_boxed_slice)
                .unwrap_or_default()
            })
            .collect();
        // Prepend the section marker only if there is data here that we wanna prepend
        if !code_tabs.is_empty() {
            code_tabs = SectionType::Lua.with_data(code_tabs);
        }
        let iter = iter.chain(code_tabs);

        // 3. Gfx
        let Asset { asset_data, .. } = gfx;
        let gfx: Box<[u8]> = SectionType::Gfx.with_data(asset_data.into_owned());
        let iter = iter.chain(gfx);

        // 4. Label (based on experimentation)
        let label: Box<[u8]> = label
            .map(|Label { label_data, .. }| SectionType::Label.with_data(label_data.into_owned()))
            .unwrap_or_default();
        let iter = iter.chain(label);

        // 5. Gff
        let gff: Box<[u8]> = gff
            .map(|Asset { asset_data, .. }| SectionType::Gff.with_data(asset_data.into_owned()))
            .unwrap_or_default();
        let iter = iter.chain(gff);

        // 6. Map
        let map: Box<[u8]> = map
            .map(|Asset { asset_data, .. }| SectionType::Map.with_data(asset_data.into_owned()))
            .unwrap_or_default();
        let iter = iter.chain(map);

        // 7. Sfx
        let sfx: Box<[u8]> = sfx
            .map(|Asset { asset_data, .. }| SectionType::Sfx.with_data(asset_data.into_owned()))
            .unwrap_or_default();
        let iter = iter.chain(sfx);

        // 8. Music
        let music: Box<[u8]> = music
            .map(|Asset { asset_data, .. }| SectionType::Music.with_data(asset_data.into_owned()))
            .unwrap_or_default();
        let iter = iter.chain(music);

        // Collect finally
        iter.collect()
    }
}

#[derive(Debug, Default)]
struct CartDataBuilder<'a> {
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

impl<'a> FromIterator<Section<'a>> for CartDataBuilder<'a> {
    fn from_iter<T: IntoIterator<Item = Section<'a>>>(iter: T) -> Self {
        iter.into_iter()
            .fold(Default::default(), |acc, section| match section {
                Section::Lua {
                    line_number,
                    section_data,
                } => {
                    let Cow::Borrowed(section_data) = section_data else {
                        panic!("Why is there a vector here");
                    };

                    CartDataBuilder {
                        code_tabs: get_code_tabs_from_lua_section(line_number, section_data),
                        ..acc
                    }
                }
                Section::Gfx {
                    line_number,
                    section_data,
                } => CartDataBuilder {
                    gfx: Some(Asset {
                        line_number,
                        asset_data: section_data,
                    }),
                    ..acc
                },
                Section::Gff {
                    line_number,
                    section_data,
                } => CartDataBuilder {
                    gff: Some(Asset {
                        line_number,
                        asset_data: section_data,
                    }),
                    ..acc
                },
                Section::Sfx {
                    line_number,
                    section_data,
                } => CartDataBuilder {
                    sfx: Some(Asset {
                        line_number,
                        asset_data: section_data,
                    }),
                    ..acc
                },
                Section::Map {
                    line_number,
                    section_data,
                } => CartDataBuilder {
                    map: Some(Asset {
                        line_number,
                        asset_data: section_data,
                    }),
                    ..acc
                },
                Section::Music {
                    line_number,
                    section_data,
                } => CartDataBuilder {
                    music: Some(Asset {
                        line_number,
                        asset_data: section_data,
                    }),
                    ..acc
                },
                Section::Label {
                    line_number,
                    section_data,
                } => CartDataBuilder {
                    label: Some(Label {
                        line_number,
                        label_data: section_data,
                    }),
                    ..acc
                },
            })
    }
}

impl<'a> CartDataBuilder<'a> {
    /// requires header to start
    fn build_with(self, header: &'a Header) -> Option<CartData<'a>> {
        let CartDataBuilder {
            label,
            gfx,
            gff,
            map,
            sfx,
            music,
            code_tabs,
        } = self;
        Some(CartData {
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
