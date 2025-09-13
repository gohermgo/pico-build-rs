//! # `pico-8-cart-builder`
//!
//! - [`CartBuilder`][`CartBuilder`]: Main '_compiler implementation_'

use std::ffi;
use std::io;
use std::path;
use std::{borrow::Cow, fs};

/// Constructs/compiles pico-8 carts
#[derive(Debug)]
pub struct CartBuilder {
    src_dir: path::PathBuf,
}

#[tracing::instrument(level = "debug", skip(path), ret)]
fn get_files_in_directory<P: AsRef<path::Path> + ?Sized>(
    path: &P,
) -> io::Result<impl Iterator<Item = fs::DirEntry>> {
    fs::read_dir(path).map(|read_dir| read_dir.filter_map(Result::ok))
}

#[tracing::instrument(level = "debug", skip(target_extension))]
fn dir_entry_has_extension<O: AsRef<ffi::OsStr> + ?Sized>(
    dir_entry: &fs::DirEntry,
    target_extension: &O,
) -> bool {
    dir_entry
        .path()
        .extension()
        .is_some_and(|extension| extension.eq(target_extension.as_ref()))
}

#[tracing::instrument(level = "debug", skip(target_extension))]
fn dir_entry_extension_filter<O: AsRef<ffi::OsStr> + ?Sized>(
    target_extension: &O,
) -> impl FnMut(&fs::DirEntry) -> bool {
    |dir_entry| dir_entry_has_extension(dir_entry, target_extension)
}

#[tracing::instrument(level = "debug", skip(path, target_extension))]
fn get_files_in_directory_with_extension<
    P: AsRef<path::Path> + ?Sized,
    O: AsRef<ffi::OsStr> + ?Sized,
>(
    path: &P,
    target_extension: &O,
) -> io::Result<impl Iterator<Item = fs::DirEntry>> {
    get_files_in_directory(path)
        .map(|files| files.filter(dir_entry_extension_filter(target_extension)))
}

/// Returns all files in the directory specified
#[tracing::instrument(level = "debug", skip(path))]
pub fn get_lua_files<P: AsRef<path::Path> + ?Sized>(
    path: &P,
) -> io::Result<impl Iterator<Item = fs::DirEntry>> {
    get_files_in_directory_with_extension(path, "lua")
    // fs::read_dir(p).map(|read_dir| {
    //     tracing::debug!("read_dir={read_dir:?}");
    //     read_dir.filter_map(|entry| {
    //         let Ok(entry) = entry else {
    //             return None;
    //         };

    //         let entry_path = entry.path();
    //         let extension_opt = entry_path.extension();

    //         if extension_opt.is_none() | extension_opt.is_some_and(|extension| extension != "lua") {
    //             return None;
    //         };

    //         Some(entry)
    //     })
    // })
}

fn dir_entries_to_source_files(
    dir_entries: impl Iterator<Item = fs::DirEntry>,
) -> impl Iterator<Item = SourceFile> {
    dir_entries.filter_map(|file| SourceFile::try_from(file).ok())
}

/// Returns an iterator of lua source-files
#[tracing::instrument(level = "debug", skip(path))]
fn source_files_in_directory<P: AsRef<path::Path> + ?Sized>(
    path: &P,
    // target_extension: &O,
) -> io::Result<impl Iterator<Item = SourceFile>> {
    get_files_in_directory_with_extension(path, "lua").map(dir_entries_to_source_files)
}

#[tracing::instrument(level = "debug", skip(source_files))]
fn source_files_to_tabs(
    source_files: impl Iterator<Item = SourceFile>,
) -> impl Iterator<Item = pico_8_cart_model::Tab<'static>> {
    let mut line_number = 0;
    source_files.map(
        move |SourceFile {
                  src_entry,
                  file_data,
              }| {
            tracing::debug!("Currently processing file {:?}", src_entry);
            let lines_in_file = bytes::NewlineIter::from(file_data.as_ref()).count();
            let section = pico_8_cart_model::Tab {
                line_number,
                code_data: Cow::Owned(
                    SourceFile {
                        src_entry,
                        file_data,
                    }
                    .collect_into(),
                ),
            };
            line_number += lines_in_file;
            section
        },
    )
}

#[tracing::instrument(level = "debug", skip(tabs))]
pub fn compile_tabs<'a>(
    tabs: impl Iterator<Item = pico_8_cart_model::Tab<'a>>,
) -> pico_8_cart_model::CodeTabs<'a> {
    tabs.enumerate()
        .fold(Default::default(), |mut tabs, (tab_index, code_tab)| {
            tracing::info!("compiling tab {tab_index}");
            tabs[tab_index] = Some(code_tab);
            tabs
        })
}

pub fn compile_tabs_to_cart_data<'a>(
    tabs: impl Iterator<Item = pico_8_cart_model::Tab<'a>>,
) -> pico_8_cart_model::CartData<'a> {
    pico_8_cart_model::CartData::default_with_code_tabs(compile_tabs(tabs))
}

#[tracing::instrument(level = "debug", skip(source_files))]
fn compile_source_files_to_tabs(
    source_files: impl Iterator<Item = SourceFile>,
) -> pico_8_cart_model::CodeTabs<'static> {
    compile_tabs(source_files_to_tabs(source_files))
}

#[tracing::instrument(level = "debug", skip(dir_entries))]
pub fn dir_entries_to_tabs(
    dir_entries: impl Iterator<Item = fs::DirEntry>,
) -> impl Iterator<Item = pico_8_cart_model::Tab<'static>> {
    source_files_to_tabs(dir_entries_to_source_files(dir_entries))
}

/// Each item in the iterator corresponds to the code
/// from one file in the directory
///
/// This function treats each file in the directory as a tab
pub fn get_tab_data_from_files_in_directory<P: AsRef<path::Path> + ?Sized>(
    path: &P,
) -> io::Result<impl Iterator<Item = pico_8_cart_model::Tab<'static>>> {
    tracing::debug!(
        "Traversing directory {:?} for lua-source files",
        path.as_ref()
    );
    source_files_in_directory(path).map(source_files_to_tabs)
}
pub fn merge_tabs_with_src<'a, P: AsRef<path::Path> + ?Sized>(
    path: &P,
    tabs: impl Iterator<Item = pico_8_cart_model::Tab<'a>>,
) -> io::Result<pico_8_cart_model::CartData<'a>> {
    // Extract tabs from files in source directory
    //
    // TODO: Abstraction for getting tabs
    //
    // This way we can traverse different file-system types
    //
    // - The one with subfolders each mapping to a tab
    // - The flat one, where a file maps to a tab (and folders are ignored as part of general filesystem)
    //   - This one might abtually be more pico-8 friendly
    let code_tabs = compile_tabs(tabs);

    // // Store tab-data in the fixed array
    // let mut code_tabs: pico_8_cart_model::CodeTabs<'_> = Default::default();
    // let mut code_tab_count = 0;
    // for (tab_index, tab) in tabs.enumerate() {
    //     tracing::debug!("Buffering tab {tab_index} {tab:?}");
    //     code_tabs[tab_index] = Some(tab);
    //     code_tab_count += 1;
    // }

    let code_tab_count = code_tabs.iter().filter(|elt| elt.is_some()).count();
    tracing::info!("Building from {code_tab_count} tabs");

    // We don't want to ignore the content of the pico-8 `main`
    // file and overwrite it without modifying it first
    //
    // TODO: Merge code-sections (might be really really complicated)
    let mut cart = pico_8_cart_model::CartData::from_file_or_default(path)?;

    // Overwrite the cart-data and recopy it
    if code_tabs.iter().any(Option::is_some) {
        cart.set_code_data(code_tabs);
    }
    Ok(cart)
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
        // Extract tabs from files in source directory
        //
        // TODO: Abstraction for getting tabs
        //
        // This way we can traverse different file-system types
        //
        // - The one with subfolders each mapping to a tab
        // - The flat one, where a file maps to a tab (and folders are ignored as part of general filesystem)
        //   - This one might abtually be more pico-8 friendly
        let code_tabs = self.get_tabs().map(compile_tabs)?;

        // // Store tab-data in the fixed array
        // let mut code_tabs: pico_8_cart_model::CodeTabs<'_> = Default::default();
        // let mut code_tab_count = 0;
        // for (tab_index, tab) in tabs.enumerate() {
        //     tracing::debug!("Buffering tab {tab_index} {tab:?}");
        //     code_tabs[tab_index] = Some(tab);
        //     code_tab_count += 1;
        // }

        let code_tab_count = code_tabs.iter().filter(|elt| elt.is_some()).count();
        tracing::info!("Building from {code_tab_count} tabs");

        // We don't want to ignore the content of the pico-8 `main`
        // file and overwrite it without modifying it first
        //
        // TODO: Merge code-sections (might be really really complicated)
        let mut cart = pico_8_cart_model::CartData::from_file_or_default(src_file)?;

        // Overwrite the cart-data and recopy it
        if code_tabs.iter().any(Option::is_some) {
            cart.set_code_data(code_tabs);
        }
        Ok(cart)
    }
}

struct SourceFile {
    src_entry: fs::DirEntry,
    file_data: Box<[u8]>,
}
impl SourceFile {
    pub fn has_extension<O: AsRef<ffi::OsStr> + ?Sized>(&self, target_extension: &O) -> bool {
        dir_entry_has_extension(&self.src_entry, target_extension)
    }
    pub fn is_lua_file(&self) -> bool {
        self.has_extension("lua")
    }
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
        let mut file = fs::File::open(value.path())
            .inspect_err(|e| tracing::error!("failed to open file as source-file: {e}"))?;

        let mut buf = vec![];
        io::Read::read_to_end(&mut file, &mut buf)
            .inspect_err(|e| tracing::error!("failed to read source-file to end: {e}"))?;

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
