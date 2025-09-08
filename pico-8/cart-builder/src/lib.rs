//! # `pico-8-cart-builder`
//!
//! - [`CartBuilder`][`CartBuilder`]: Main '_compiler implementation_'

use std::io;
use std::path;
use std::{borrow::Cow, fs};

/// Constructs/compiles pico-8 carts
#[derive(Debug)]
pub struct CartBuilder {
    src_dir: path::PathBuf,
}

#[tracing::instrument(level = "debug", skip(p), ret)]
fn get_lua_files<P: AsRef<path::Path> + ?Sized>(
    p: &P,
) -> std::io::Result<impl Iterator<Item = fs::DirEntry>> {
    fs::read_dir(p).map(|read_dir| {
        read_dir.filter_map(|entry| {
            let Ok(entry) = entry else {
                return None;
            };
            let entry_path = entry.path();
            let extension_opt = entry_path.extension();
            if extension_opt.is_none() | extension_opt.is_some_and(|extension| extension != "lua") {
                return None;
            };
            Some(entry)
        })
    })
}

impl CartBuilder {
    #[tracing::instrument(level = "debug", skip(src_dir) ret)]
    pub fn new<P: AsRef<path::Path> + ?Sized>(src_dir: &P) -> CartBuilder {
        CartBuilder {
            src_dir: src_dir.as_ref().to_path_buf(),
        }
    }

    #[tracing::instrument(level = "debug")]
    fn file_builders(&self) -> std::io::Result<impl Iterator<Item = SourceFile>> {
        let path = self.src_dir.as_path();
        get_lua_files(path).map(|lua_files| {
            lua_files.filter_map(|file| {
                SourceFile::try_from(file)
                    .inspect_err(|e| tracing::warn!("failed to open lua file {e}"))
                    .ok()
            })
        })
    }
    #[tracing::instrument(level = "debug")]
    pub fn get_tabs(&self) -> io::Result<impl Iterator<Item = pico_8_cart_model::Tab<'static>>> {
        tracing::debug!("Getting tabs");
        let mut line_number = 0;
        self.file_builders().map(|files| {
            files.map(move |elt| {
                tracing::debug!("{line_number}");
                let slice = Cow::Owned(elt.collect_into());
                let lines_in_file = bytes::NewlineIter::from(&slice).count();
                let section: pico_8_cart_model::Tab<'static> = pico_8_cart_model::Tab {
                    line_number,
                    code_data: slice,
                };
                line_number += lines_in_file;
                section
            })
        })
    }
    #[tracing::instrument(level = "debug", skip(src_file), ret)]
    pub fn build<P: AsRef<path::Path> + ?Sized>(
        &self,
        src_file: &P,
    ) -> io::Result<pico_8_cart_model::CartData<'static>> {
        let tabs = self.get_tabs()?;
        let mut code_tabs: pico_8_cart_model::CodeTabs<'_> = Default::default();
        for (tab_index, tab) in tabs.enumerate() {
            tracing::debug!("Buffering tab {tab_index} {tab:?}");
            code_tabs[tab_index] = Some(tab);
        }
        if src_file.as_ref().exists() {
            let mut cart_source = vec![];
            let mut cart_file = fs::File::open(src_file)?;

            io::Read::read_to_end(&mut cart_file, &mut cart_source)?;

            let mut cart = pico_8_cart_model::CartData::from_cart_source(cart_source.as_slice())?;
            // Overwrite the cart-data and recopy it
            if code_tabs.iter().any(Option::is_some) {
                cart.set_code_data(code_tabs);
            }
            Ok(cart.into_owned())
        } else {
            const DEFAULT_HEADER: &pico_8_cart_model::Header = unsafe {
                core::mem::transmute(
                    br"pico-8 cartridge // http://www.pico-8.com
version 43
"
                    .as_slice(),
                )
            };
            const DEFAULT_GFX: &[u8] = br"__gfx__
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
            Ok(pico_8_cart_model::CartData::from_parts(
                DEFAULT_HEADER,
                code_tabs,
                DEFAULT_GFX,
            ))
        }
    }
}

struct SourceFile {
    src_entry: fs::DirEntry,
    file_data: Box<[u8]>,
}
// impl From<fs::DirEntry> for FileBuilder {
//     fn from(value: fs::DirEntry) -> Self {
//         FileBuilder { src_entry: value }
//     }
// }
impl TryFrom<fs::DirEntry> for SourceFile {
    type Error = io::Error;
    #[tracing::instrument(level = "debug")]
    fn try_from(value: fs::DirEntry) -> Result<Self, Self::Error> {
        let mut file = fs::File::open(value.path())?;

        let mut buf = vec![];
        io::Read::read_to_end(&mut file, &mut buf)?;

        Ok(SourceFile {
            src_entry: value,
            file_data: buf.into_boxed_slice(),
        })
    }
}
impl SourceFile {
    fn get_name(&self) -> String {
        self.src_entry
            .file_name()
            .into_string()
            .expect("pico-8 source file name contained invalid utf-8")
    }

    #[tracing::instrument(level = "debug", skip(self))]
    #[inline(always)]
    fn collect_into<T: FromIterator<u8>>(self) -> T {
        if self.file_data.starts_with(b"--") {
            self.file_data.into_iter().collect()
        } else {
            let name = self.get_name();
            let SourceFile { file_data, .. } = self;
            // So the pico-8 editor gets a nice title view too, QoL i guess...
            let name_as_comment = format!("-- {name}\n");
            name_as_comment
                .into_bytes()
                .into_iter()
                .chain(file_data)
                .collect()
        }
    }

    #[expect(dead_code)]
    fn into_boxed_slice(self) -> Box<[u8]> {
        if self.file_data.starts_with(b"--") {
            self.file_data
        } else {
            let name = self.get_name();
            let SourceFile { file_data, .. } = self;
            // So the pico-8 editor gets a nice title view too, QoL i guess...
            let name_as_comment = format!("-- {name}\n");
            name_as_comment
                .into_bytes()
                .into_iter()
                .chain(file_data)
                .collect()
        }
    }
}
