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
#[tracing::instrument(level = "debug", skip(directory_path))]
pub fn get_lua_files<P: AsRef<path::Path> + ?Sized>(
    directory_path: &P,
) -> io::Result<impl Iterator<Item = fs::DirEntry>> {
    get_files_in_directory_with_extension(directory_path, "lua")
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

// impl CartBuilder {
//     #[tracing::instrument(level = "debug", skip(src_dir) ret)]
//     pub fn new<P: AsRef<path::Path> + ?Sized>(src_dir: &P) -> CartBuilder {
//         CartBuilder {
//             src_dir: src_dir.as_ref().to_path_buf(),
//         }
//     }

//     #[tracing::instrument(level = "debug")]
//     fn file_builders(&self) -> std::io::Result<impl Iterator<Item = SourceFile>> {
//         let path = self.src_dir.as_path();
//         get_lua_files(path).map(|lua_files| {
//             lua_files.filter_map(|file| {
//                 SourceFile::try_from(file)
//                     .inspect_err(|e| tracing::warn!("failed to open lua file {e}"))
//                     .ok()
//             })
//         })
//     }
//     #[tracing::instrument(level = "debug")]
//     pub fn get_tabs(&self) -> io::Result<impl Iterator<Item = pico_8_cart_model::Tab<'static>>> {
//         tracing::debug!("Getting tabs");
//         let mut line_number = 0;
//         self.file_builders().map(|files| {
//             files.map(move |elt| {
//                 tracing::debug!("{line_number}");
//                 let slice = Cow::Owned(elt.collect_into());
//                 let lines_in_file = bytes::NewlineIter::from(&slice).count();
//                 let section: pico_8_cart_model::Tab<'static> = pico_8_cart_model::Tab {
//                     line_number,
//                     code_data: slice,
//                 };
//                 line_number += lines_in_file;
//                 section
//             })
//         })
//     }
//     #[tracing::instrument(level = "debug", skip(src_file), ret)]
//     pub fn build<P: AsRef<path::Path> + ?Sized>(
//         &self,
//         src_file: &P,
//     ) -> io::Result<pico_8_cart_model::CartData<'static>> {
//         // Extract tabs from files in source directory
//         //
//         // TODO: Abstraction for getting tabs
//         //
//         // This way we can traverse different file-system types
//         //
//         // - The one with subfolders each mapping to a tab
//         // - The flat one, where a file maps to a tab (and folders are ignored as part of general filesystem)
//         //   - This one might abtually be more pico-8 friendly
//         let code_tabs = self.get_tabs().map(compile_tabs)?;

//         // // Store tab-data in the fixed array
//         // let mut code_tabs: pico_8_cart_model::CodeTabs<'_> = Default::default();
//         // let mut code_tab_count = 0;
//         // for (tab_index, tab) in tabs.enumerate() {
//         //     tracing::debug!("Buffering tab {tab_index} {tab:?}");
//         //     code_tabs[tab_index] = Some(tab);
//         //     code_tab_count += 1;
//         // }

//         let code_tab_count = code_tabs.iter().filter(|elt| elt.is_some()).count();
//         tracing::info!("Building from {code_tab_count} tabs");

//         // We don't want to ignore the content of the pico-8 `main`
//         // file and overwrite it without modifying it first
//         //
//         // TODO: Merge code-sections (might be really really complicated)
//         let mut cart = pico_8_cart_model::CartData::from_file_or_default(src_file)?;

//         // Overwrite the cart-data and recopy it
//         if code_tabs.iter().any(Option::is_some) {
//             cart.set_code_data(code_tabs);
//         }
//         Ok(cart)
//     }
// }
