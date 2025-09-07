extern crate alloc;

use core::fmt;
use core::mem::transmute;

use alloc::borrow::Cow;

use std::io;

mod bytes;

mod header;

mod section;
pub use section::{Section, SectionData, SectionDataBuf, SectionDelimiter, SectionType};

use crate::header::Header;

#[derive(Debug)]
struct P8CartData<'a> {
    lua: Section<'a>,
    gfx: Section<'a>,
    gff: Option<Section<'a>>,
    /// Label is optional
    label: Option<Section<'a>>,
    map: Option<Section<'a>>,
    sfx: Option<Section<'a>>,
    music: Option<Section<'a>>,
}

/// Returns the section-delimiters ordered by line-number
#[tracing::instrument(skip(cart_src), ret)]
fn get_section_delimiters(
    cart_src: &[u8],
    line_number_offset: Option<usize>,
) -> Vec<SectionDelimiter<'static>> {
    let mut offset = 0;
    // add 1 to compensate non-zero start of file
    let line_number_offset = line_number_offset.unwrap_or_default() + 1;
    let mut section_delimiters: Vec<SectionDelimiter<'static>> = bytes::NewlineIter::new(cart_src)
        .enumerate()
        .filter_map(|(line_number, line)| {
            let delimiter = section::get_line_type(line).map(|ty| {
                tracing::debug!(
                    "Section of {ty:?} starts at {line_number}: {:?}",
                    core::str::from_utf8(line)
                );
                if matches!(ty, SectionType::Lua) {
                    tracing::debug!("Got lua-line {line:#?}");
                }
                SectionDelimiter {
                    ty,
                    line_number: line_number + line_number_offset,
                    offset,
                }
            });
            if delimiter.is_none() {
                tracing::debug!("{line_number}: {:?}", core::str::from_utf8(line));
            }
            offset += line.len();
            delimiter
        })
        .collect();
    // Sort, so that sections are in order of line-number
    section_delimiters.sort();
    section_delimiters
}

impl<'a> P8CartData<'a> {
    #[tracing::instrument(skip(cart_src))]
    fn get_from_lines(
        cart_src: &'a [u8],
        line_number_offset: Option<usize>,
        // byte_offset: Option<usize>,
    ) -> io::Result<P8CartData<'a>> {
        // This gives us the file-sections in sorted-order by line-number
        let mut section_delimiters = get_section_delimiters(cart_src, line_number_offset);
        // Reverse, so we can traverse backwards and use an external
        // variable to track previous (next actually, since reverse)
        // section length
        section_delimiters.reverse();
        let mut next_section_offset = 0;
        let sections = section_delimiters.into_iter().enumerate().filter_map(
            |(
                idx,
                SectionDelimiter {
                    ty,
                    offset,
                    line_number,
                },
            )| {
                let ty_str = <&'static str as From<&SectionType>>::from(ty);
                // Filter out the type-marker + `\n` before converting into section data
                //
                // We can always reverse the slice provided we need to recover it
                // (and still in borrowed cow-state)
                // let section_src = cart_src.get(ty_str.len() + 1..)?;
                let offset = offset + ty_str.len() + 1;
                let section_src = if idx == 0 {
                    cart_src.get(offset..)
                } else {
                    cart_src.get(offset..next_section_offset)
                }?;
                if matches!(ty, SectionType::Lua) {
                    tracing::debug!("Here is lua {:#?}", core::str::from_utf8(section_src));
                }

                next_section_offset = offset;

                let section_data: &SectionData = unsafe { transmute(section_src) };
                let section = Section {
                    ty,
                    line_number,
                    data: Cow::Borrowed(section_data),
                };
                tracing::debug!(
                    "[Line: {line_number:>4} | Size: {:>6} | Offset: {offset:>6} -> {:>6}] {ty:?}",
                    section_src.len(),
                    offset + section_src.len()
                );
                Some(section)
            },
        );
        #[derive(Debug, Default)]
        struct P8CartDataBuilder<'a> {
            lua: Option<Section<'a>>,
            gfx: Option<Section<'a>>,
            gff: Option<Section<'a>>,
            label: Option<Section<'a>>,
            map: Option<Section<'a>>,
            sfx: Option<Section<'a>>,
            music: Option<Section<'a>>,
        }

        impl<'a> FromIterator<Section<'a>> for P8CartDataBuilder<'a> {
            fn from_iter<T: IntoIterator<Item = Section<'a>>>(iter: T) -> Self {
                iter.into_iter().fold(
                    P8CartDataBuilder::default(),
                    |acc, section @ Section { ty, .. }| match ty {
                        SectionType::Lua => P8CartDataBuilder {
                            lua: Some(section),
                            ..acc
                        },
                        SectionType::Gfx => P8CartDataBuilder {
                            gfx: Some(section),
                            ..acc
                        },
                        SectionType::Gff => P8CartDataBuilder {
                            gff: Some(section),
                            ..acc
                        },
                        SectionType::Label => P8CartDataBuilder {
                            label: Some(section),
                            ..acc
                        },
                        SectionType::Map => P8CartDataBuilder {
                            map: Some(section),
                            ..acc
                        },
                        SectionType::Sfx => P8CartDataBuilder {
                            sfx: Some(section),
                            ..acc
                        },
                        SectionType::Music => P8CartDataBuilder {
                            music: Some(section),
                            ..acc
                        },
                    },
                )
            }
        }

        impl<'a> P8CartDataBuilder<'a> {
            #[tracing::instrument(level = "debug")]
            fn build(self) -> Option<P8CartData<'a>> {
                tracing::debug!("Building cart-data");
                let P8CartDataBuilder {
                    lua,
                    gfx,
                    gff,
                    label,
                    map,
                    sfx,
                    music,
                } = self;
                Some(P8CartData {
                    lua: lua?,
                    gfx: gfx?,
                    gff,
                    label,
                    map,
                    sfx,
                    music,
                })
            }
        }

        Ok(P8CartDataBuilder::from_iter(sections)
            .build()
            .expect("Failed to build cart-data"))
    }
    fn into_owned(self) -> P8CartData<'static> {
        let P8CartData {
            lua,
            gfx,
            gff,
            label,
            map,
            sfx,
            music,
        } = self;
        P8CartData {
            lua: lua.into_owned(),
            gfx: gfx.into_owned(),
            gff: gff.map(Section::into_owned),
            label: label.map(Section::into_owned),
            map: map.map(Section::into_owned),
            sfx: sfx.map(Section::into_owned),
            music: music.map(Section::into_owned),
        }
    }
}

#[derive(Debug)]
struct P8AssetData<'a> {
    gfx: Section<'a>,
    gff: Option<Section<'a>>,
    map: Option<Section<'a>>,
    sfx: Option<Section<'a>>,
    music: Option<Section<'a>>,
}

impl P8AssetData<'_> {
    fn into_owned(self) -> P8AssetData<'static> {
        let P8AssetData {
            gfx,
            gff,
            map,
            sfx,
            music,
        } = self;
        P8AssetData {
            gfx: gfx.into_owned(),
            gff: gff.map(Section::into_owned),
            map: map.map(Section::into_owned),
            sfx: sfx.map(Section::into_owned),
            music: music.map(Section::into_owned),
        }
    }
}

const P8_MAX_CODE_EDITOR_TAB_COUNT: usize = 15;

/// Always of the `lua` type
struct Tab<'a> {
    line_number: usize,
    code: Cow<'a, SectionData>,
}

impl Tab<'_> {
    fn into_owned(self) -> Tab<'static> {
        let Tab { line_number, code } = self;
        Tab {
            line_number,
            code: Cow::Owned(code.into_owned()),
        }
    }
}

impl fmt::Debug for Tab<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tab")
            .field("line_number", &self.line_number)
            .field("code", &String::from_utf8_lossy(&self.code.0))
            .finish()
    }
}

#[derive(Debug, Default)]
struct P8CodeData<'a> {
    tabs: [Option<Tab<'a>>; P8_MAX_CODE_EDITOR_TAB_COUNT],
}

impl<'a> P8CodeData<'a> {
    #[tracing::instrument(level = "debug", skip(section_data), ret)]
    fn from_lua_section_v2(
        mut line_number: usize,
        section_data: &'a SectionData,
    ) -> P8CodeData<'a> {
        let mut tabs = <[Option<Tab<'a>>; P8_MAX_CODE_EDITOR_TAB_COUNT]>::default();
        let tab_iter = bytes::TabIter::new(&section_data.0);
        for (idx, tab) in tab_iter.enumerate() {
            if idx != 0 {
                line_number += 1;
            };
            line_number = if idx == 0 {
                line_number
            } else {
                // Have to increment past the tab-separator
                line_number + 1
            };
            tracing::debug!("Tab-index: {idx}: {:?}", core::str::from_utf8(tab));
            let section_data: &'a SectionData = unsafe { transmute(tab) };
            tabs[idx] = Some(Tab {
                line_number,
                code: Cow::Borrowed(section_data),
            });
            let lines_in_section = bytes::NewlineIter::new(tab).count();
            line_number += lines_in_section;
        }
        P8CodeData { tabs }
    }
    #[tracing::instrument(level = "debug", skip(section_data))]
    fn from_lua_section(
        mut line_number: usize,
        mut section_data: &'a SectionData,
    ) -> P8CodeData<'a> {
        let mut tabs = <[Option<Tab<'a>>; P8_MAX_CODE_EDITOR_TAB_COUNT]>::default();
        // assert!(
        //     matches!(section.ty, SectionType::Lua),
        //     "Attempted tabulation of non-lua section"
        // );

        const TAB_SEQUENCE: &[u8] = b"-->8";
        let seqs = section_data
            .0
            .windows(TAB_SEQUENCE.len())
            .filter(|window| (*window).eq(TAB_SEQUENCE))
            .count();
        tracing::info!("There should be {seqs} tab-sequences");

        if section_data
            .split_at_sequence_exclusive(TAB_SEQUENCE)
            .is_none()
        {
            tracing::debug!("Only made a single tab");
            tabs[0] = Some(Tab {
                line_number,
                code: Cow::Borrowed(section_data),
            });
            return P8CodeData { tabs };
        }

        let mut tab_idx = 0;
        while let Some((code, remainder)) = section_data
            .split_at_sequence_exclusive(TAB_SEQUENCE)
            .map(|(code, remainder)| (Cow::Borrowed(code), remainder))
        {
            // 1 count line_number
            let newline_count = code.0.iter().filter(|b| matches!(b, b'\n')).count();
            tracing::debug!("Got {newline_count} newlines");
            tracing::debug!(
                "Here is remainder {:#?}",
                core::str::from_utf8(&remainder.0)
            );
            line_number += newline_count;
            // 2 push data at index
            tabs[tab_idx] = Some(Tab { line_number, code });

            // 3 update for next iteration
            tab_idx += 1;
            section_data = remainder;
        }
        // Since we assigned to `data` within the loop, it will be the last section
        // 1 count line_number in the remainder `data`
        let newline_count = section_data.0.iter().filter(|b| matches!(b, b'\n')).count();
        line_number += newline_count;
        // 2 push data at index (index has been pushed appropriately)
        tabs[tab_idx] = Some(Tab {
            line_number,
            code: Cow::Borrowed(section_data),
        });

        // // Check if we can split a single
        // if let Some((fst, remainder)) = section_data.split_at_sequence_exclusive(TAB_SEQUENCE) {
        //     tracing::debug!("Managed first split");
        //     tabs[0] = Some(Tab {
        //         line_number,
        //         code: Cow::Borrowed(fst),
        //     });

        //     let mut tab_idx = 1;

        //     section_data = remainder;

        //     while let Some((code, remainder)) = section_data
        //         .split_at_sequence_exclusive(TAB_SEQUENCE)
        //         .map(|(code, remainder)| (Cow::Borrowed(code), remainder))
        //     {
        //         // 1 count line_number
        //         let newline_count = code.0.iter().filter(|b| matches!(b, b'\n')).count();
        //         tracing::debug!("Got {newline_count} newlines");
        //         line_number += newline_count;
        //         // 2 push data at index
        //         tabs[tab_idx] = Some(Tab { line_number, code });

        //         // 3 update for next iteration
        //         tab_idx += 1;
        //         section_data = remainder;
        //     }

        //     // Since we assigned to `data` within the loop, it will be the last section
        //     // 1 count line_number in the remainder `data`
        //     let newline_count = remainder.0.iter().filter(|b| matches!(b, b'\n')).count();
        //     line_number += newline_count;
        //     // 2 push data at index (index has been pushed appropriately)
        //     tabs[tab_idx] = Some(Tab {
        //         line_number,
        //         code: Cow::Borrowed(remainder),
        //     });
        // } else {
        //     tabs[0] = Some(Tab {
        //         line_number,
        //         code: Cow::Borrowed(section_data),
        //     });
        // };

        tracing::debug!(
            "Made {} tabs",
            tabs.iter().filter(|elt| elt.is_some()).count()
        );

        P8CodeData { tabs }
    }
    fn into_owned(self) -> P8CodeData<'static> {
        let P8CodeData { tabs } = self;
        let mut owned_tabs = <[Option<Tab<'_>>; P8_MAX_CODE_EDITOR_TAB_COUNT]>::default();
        for (tab_idx, tab_section) in tabs.into_iter().enumerate() {
            owned_tabs[tab_idx] = tab_section.map(Tab::into_owned);
        }
        P8CodeData { tabs: owned_tabs }
    }
}

#[derive(Debug)]
pub struct P8Cart<'a> {
    header: Cow<'a, Header>,
    /// Label is optional
    label: Option<Section<'a>>,
    asset_data: P8AssetData<'a>,
    code_data: P8CodeData<'a>,
}

impl<'a> P8Cart<'a> {
    fn into_owned(self) -> P8Cart<'static> {
        let P8Cart {
            header,
            label,
            asset_data,
            code_data,
        } = self;
        P8Cart {
            header: Cow::Owned(header.into_owned()),
            label: label.map(Section::into_owned),
            asset_data: asset_data.into_owned(),
            code_data: code_data.into_owned(),
        }
    }
    /// Delegates to [`P8Cart::from_cart_source`]
    ///
    /// As implied by the return-type, the returned [`P8Cart`]
    /// is owned following parsing.
    ///
    /// May not end up being the final implementation
    pub fn try_from_reader<R: io::Read>(
        cart_file: &mut R,
    ) -> Result<P8Cart<'static>, Box<dyn core::error::Error>> {
        // Buffer for file-data
        let mut cart_src = vec![];

        // Copy file-data
        io::Read::read_to_end(cart_file, &mut cart_src)?;

        // Delegate to simpler function only handling parsing
        P8Cart::from_cart_source(cart_src.as_slice()).map(P8Cart::into_owned)
    }

    /// Attempts to parse the provided cart-source
    pub fn from_cart_source(cart_src: &'a [u8]) -> Result<P8Cart<'a>, Box<dyn core::error::Error>> {
        let (header, remainder) =
            header::split_from(cart_src).expect("failed to split cart-header");
        // let (_newline, remainder) = remainder.split_first().unwrap();

        let P8CartData {
            lua:
                Section {
                    line_number: code_line_number,
                    data: code_data,
                    ..
                },
            gfx,
            gff,
            label,
            map,
            sfx,
            music,
        } = P8CartData::get_from_lines(remainder, Some(2))?;

        let Cow::Borrowed(code_section_data) = code_data else {
            unsafe { core::hint::unreachable_unchecked() }
        };

        Ok(P8Cart {
            header: Cow::Borrowed(header),
            label,
            code_data: P8CodeData::from_lua_section_v2(code_line_number, code_section_data),
            asset_data: P8AssetData {
                gfx,
                gff,
                map,
                sfx,
                music,
            },
        })
    }
}

struct P8File {}

struct P8CartReader<'a> {
    offset: usize,
    lines: io::Lines<&'a [u8]>,
}

#[cfg(test)]
mod test_data {
    /// version 43
    ///
    /// empty other than gfx
    pub(crate) const ONLY_GFX_SECTION_MAX_TABS: &str = r"";
}
