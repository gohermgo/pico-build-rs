extern crate alloc;

use core::borrow::Borrow;
use core::fmt;
use core::mem::transmute;
use core::ops::Deref;

use alloc::borrow::Cow;

use std::io;

mod bytes;

mod section;
pub use section::{Section, SectionData, SectionDataBuf, SectionDelimiter, SectionType};

#[repr(transparent)]
struct P8CartMetadata([u8]);

fn find_byte_index<T: AsRef<[u8]> + ?Sized>(src: &T, byte: u8) -> Option<usize> {
    src.as_ref()
        .iter()
        .enumerate()
        .find_map(|(idx, elt)| (*elt == byte).then_some(idx))
}

impl P8CartMetadata {
    pub fn get_as_tuple(&self) -> Option<(&[u8], &[u8])> {
        let newline_idx = find_byte_index(&self.0, b'\n')? + 1;
        Some(unsafe { self.0.split_at_unchecked(newline_idx) })
    }
    pub fn get_version(&self) -> Option<u32> {
        let (_, version_line) = self.get_as_tuple()?;

        // Find space and add 1
        let space_idx = find_byte_index(version_line, b' ')? + 1;
        let (_, version_number) = unsafe { version_line.split_at_unchecked(space_idx) };

        let version_number_str = core::str::from_utf8(version_number)
            .inspect_err(|e| tracing::error!("Failed to split version number string: {e}"))
            .ok()?;

        <u32 as core::str::FromStr>::from_str(version_number_str)
            .inspect_err(|e| tracing::error!("Failed to parse version number: {e}"))
            .ok()
    }
    #[tracing::instrument(skip(cart_src))]
    fn split_from<T: AsRef<[u8]> + ?Sized>(cart_src: &T) -> Option<(&P8CartMetadata, &[u8])> {
        // Find first newline and split version line (and add one to not include newline)
        let newline_idx_fst = find_byte_index(cart_src, b'\n')? + 1;
        let (_, version_line) = unsafe { cart_src.as_ref().split_at_unchecked(newline_idx_fst) };

        // Find second newline index
        let newline_idx_snd = find_byte_index(version_line, b'\n')? + 1;

        // Calculate length from found indices
        let metadata_length = newline_idx_fst + newline_idx_snd;

        // Finally split out and transmute slice
        cart_src
            .as_ref()
            .split_at_checked(metadata_length)
            .map(|(metadata_bytes, remainder)| {
                let metadata: &P8CartMetadata = unsafe { transmute(metadata_bytes) };
                (metadata, remainder)
            })
    }
}

impl fmt::Debug for P8CartMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some((fst, snd)) = self.get_as_tuple() {
            f.debug_struct("P8CartMetadata")
                .field(
                    "header_line",
                    &core::str::from_utf8(fst).expect("failed to convert header to utf8"),
                )
                .field(
                    "version_line",
                    &core::str::from_utf8(snd).expect("failed to convert version-line to utf8"),
                )
                .finish()
        } else {
            f.debug_tuple("P8CartMetadata")
                .field(&String::from_utf8_lossy(&self.0))
                .finish()
        }
    }
}

impl ToOwned for P8CartMetadata {
    type Owned = P8CartMetadataBuf;
    fn to_owned(&self) -> Self::Owned {
        P8CartMetadataBuf(Box::from(&self.0))
    }
}

#[repr(transparent)]
struct P8CartMetadataBuf(Box<[u8]>);

impl Deref for P8CartMetadataBuf {
    type Target = P8CartMetadata;
    fn deref(&self) -> &Self::Target {
        let slice = self.0.as_ref();
        unsafe { transmute(slice) }
    }
}

impl Borrow<P8CartMetadata> for P8CartMetadataBuf {
    fn borrow(&self) -> &P8CartMetadata {
        self
    }
}

impl fmt::Debug for P8CartMetadataBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d: &P8CartMetadata = self;
        d.fmt(f)
    }
}

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
#[tracing::instrument(skip(cart_src, line_reader), ret)]
fn get_section_delimiters<R: io::BufRead>(
    cart_src: &[u8],
    line_reader: &mut io::Lines<R>,
    line_number_offset: Option<usize>,
    byte_offset: Option<usize>,
) -> Vec<SectionDelimiter<'static>> {
    let mut offset = byte_offset.unwrap_or_default();
    // add 1 to compensate non-zero start of file
    let line_number_offset = line_number_offset.unwrap_or_default() + 1;
    let newline_indices = cart_src
        .iter()
        .enumerate()
        .filter_map(|(idx, byte)| byte.eq(&b'\n').then_some(idx));
    let mut last_newline_index = 0;
    let mut newline_offset = 0;
    let mut line_slice_buf = cart_src;
    let lines = newline_indices
        .filter_map(|newline_index| {
            let newline_index = {
                let val = newline_index - last_newline_index;
                last_newline_index = newline_index + 1;
                val
            };
            // Do +1 to skip the newline
            let (line, tail) = line_slice_buf.split_at_checked(newline_index)?;
            let (_newline, tail) = tail.split_first()?;
            line_slice_buf = tail;

            Some(line)
        })
        .inspect(|line| tracing::debug!("{:?}", core::str::from_utf8(line)));

    // let line_count = lines.count() + line_number_offset;

    // tracing::debug!("There should be {line_count} lines");

    let mut section_delimiters: Vec<SectionDelimiter<'static>> = lines
        .enumerate()
        .filter_map(|(line_number, line)| {
            // if line.contains(b"-->8") {
            //     tracing::debug!(
            //         "Got tab-line at offset={offset}, line_number={}",
            //         line_number + line_number_offset
            //     )
            // }
            let delimiter = section::get_line_type(line).map(|ty| {
                if matches!(ty, SectionType::Lua) {
                    tracing::debug!("Got lua-line {line:#?}");
                }
                SectionDelimiter {
                    ty,
                    line_number: line_number + line_number_offset,
                    offset,
                }
            });
            offset += line.len();
            delimiter
            // line_result
            //     .inspect_err(|e| {
            //         tracing::warn!("failed to get next line in cart-file: {e}");
            //     })
            //     .ok()
            //     .and_then(|line| {
            //     })
        })
        .collect();
    // let mut section_delimiters: Vec<SectionDelimiter<'static>> = line_reader
    //     .enumerate()
    //     .filter_map(|(line_number, line_result)| {
    //         line_result
    //             .inspect_err(|e| {
    //                 tracing::warn!("failed to get next line in cart-file: {e}");
    //             })
    //             .ok()
    //             .and_then(|line| {
    //                 if line.contains("-->8") {
    //                     tracing::debug!(
    //                         "Got tab-line at offset={offset}, line_number={}",
    //                         line_number + line_number_offset
    //                     )
    //                 }
    //                 let delimiter = get_line_section_type(line.as_bytes()).map(|ty| {
    //                     if matches!(ty, SectionType::Lua) {
    //                         tracing::debug!("Got lua-line {line:#?}");
    //                     }
    //                     SectionDelimiter {
    //                         ty,
    //                         line_number: line_number + line_number_offset,
    //                         offset,
    //                     }
    //                 });
    //                 offset += line.len();
    //                 delimiter
    //             })
    //     })
    //     .collect();
    // Sort, so that sections are in order of line-number
    section_delimiters.sort();
    section_delimiters
}

impl<'a> P8CartData<'a> {
    #[tracing::instrument(skip(cart_src, line_reader))]
    fn get_from_lines<R: io::BufRead>(
        cart_src: &'a [u8],
        line_reader: &mut io::Lines<R>,
        line_number_offset: Option<usize>,
        byte_offset: Option<usize>,
    ) -> io::Result<P8CartData<'a>> {
        // This gives us the file-sections in sorted-order by line-number
        let mut section_delimiters =
            get_section_delimiters(cart_src, line_reader, line_number_offset, byte_offset);
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
                let offset = offset + ty_str.len();
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

const P8_MAX_CODE_EDITOR_TAB_COUNT: usize = 8;

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
    /// Label is optional
    label: Option<Section<'a>>,
    metadata: Cow<'a, P8CartMetadata>,
    asset_data: P8AssetData<'a>,
    code_data: P8CodeData<'a>,
}

impl<'a> P8Cart<'a> {
    fn into_owned(self) -> P8Cart<'static> {
        let P8Cart {
            label,
            metadata,
            asset_data,
            code_data,
        } = self;
        P8Cart {
            label: label.map(Section::into_owned),
            metadata: Cow::Owned(metadata.into_owned()),
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
        let (metadata, remainder) =
            P8CartMetadata::split_from(cart_src).expect("failed to split cart-metadata");

        let mut buf_reader = io::BufReader::new(remainder);
        let mut line_reader = io::BufRead::lines(&mut buf_reader);

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
        } = P8CartData::get_from_lines(
            remainder,
            &mut line_reader,
            Some(2),
            Some(cart_src.len() - remainder.len()),
        )?;

        let Cow::Borrowed(code_section_data) = code_data else {
            unsafe { core::hint::unreachable_unchecked() }
        };

        Ok(P8Cart {
            metadata: Cow::Borrowed(metadata),
            label,
            code_data: P8CodeData::from_lua_section(code_line_number, code_section_data),
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
mod tests {
    use super::*;

    const TEST_DATA: &str = r"pico-8 cartridge // http://www.pico-8.com
version 43
__lua__
-- tab 0
-->8
-- tab 1
-->8
-- tab 2
-->8
-- tab 3
-->8
-- tab 4
-->8
-- tab 5
-->8
-- tab 6
-->8
-- tab 7
-->8
-- tab 8
-->8
-- tab 9
-->8
-- tab a
-->8
-- tab b
-->8
-- tab c
-->8
-- tab d
-->8
-- tab e
-->8
-- tab f
__gfx__
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
";
    #[test]
    fn parses_metadata() {}
}
