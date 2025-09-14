extern crate alloc;

use core::fmt;
use core::iter;
use core::mem::transmute;
use core::ops::Deref;
use core::slice;

use alloc::borrow::Cow;
use alloc::vec;

use std::ffi;
use std::fs;
use std::io;
use std::path;

mod section;
pub use section::{Section, SectionData, SectionDataBuf, SectionDelimiter, SectionType};

use pico_8_cart_model::header;

/// A fixed-size collection
/// acting like a `fifo`
pub struct Fifo<T> {
    inner: Box<[T]>,
    cursor: usize,
}

impl<T> Fifo<T> {
    /// Returns the value of the incremented cursor
    ///
    /// The cursor will be incremented such that
    /// it always refers to a region within the buffer
    const fn incremented_cursor_value(&self) -> usize {
        let Fifo { inner, cursor } = self;

        let naively_incremented = *cursor + 1;

        // In this first case, once incremented we point outside the buffer-region
        if naively_incremented == inner.len() {
            // Thus, we just return zero
            0
        } else {
            // In this second case, we are still bounded
            naively_incremented
        }
    }

    fn get_at_cursor_mut(&mut self) -> &mut T {
        unsafe { self.inner.get_unchecked_mut(self.cursor) }
    }

    /// Returns an iterator over a contiguous region
    /// with the contained `cursor` corresponding
    /// to the iterator-region's origin
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.into_iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.into_iter()
    }

    /// Overwrites the element pointed to by the
    /// cursor value currently.
    ///
    /// Returns the index of the written item
    ///
    pub fn overwrite(&mut self, value: T) -> usize {
        *self.get_at_cursor_mut() = value;
        let write_index = self.cursor;
        self.cursor = self.incremented_cursor_value();
        write_index
    }

    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[inline]
    pub const fn len(&self) -> usize {
        self.inner.len()
    }

    pub const fn reset_cursor(&mut self) {
        self.cursor = 0;
    }
}

impl<T> Default for Fifo<T> {
    fn default() -> Self {
        Fifo {
            inner: Box::default(),
            cursor: usize::default(),
        }
    }
}

impl<T> From<Box<[T]>> for Fifo<T> {
    fn from(value: Box<[T]>) -> Self {
        Fifo {
            inner: value,
            ..Default::default()
        }
    }
}

impl<T> FromIterator<T> for Fifo<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let inner: Box<[T]> = Box::from_iter(iter);
        Fifo::from(inner)
    }
}

impl<'a, T> IntoIterator for &'a Fifo<T> {
    type Item = &'a T;
    type IntoIter = iter::Chain<slice::Iter<'a, T>, slice::Iter<'a, T>>;
    fn into_iter(self) -> Self::IntoIter {
        let (until_cursor, cursor_and_onwards) = self.inner.split_at(self.cursor);

        cursor_and_onwards.iter().chain(until_cursor)
    }
}

impl<'a, T> IntoIterator for &'a mut Fifo<T> {
    type Item = &'a mut T;
    type IntoIter = iter::Chain<slice::IterMut<'a, T>, slice::IterMut<'a, T>>;
    fn into_iter(self) -> Self::IntoIter {
        let (until_cursor, cursor_and_onwards) = self.inner.split_at_mut(self.cursor);

        cursor_and_onwards.iter_mut().chain(until_cursor)
    }
}

impl<T> IntoIterator for Fifo<T>
where
    T: Clone,
{
    type Item = T;
    type IntoIter = iter::Chain<vec::IntoIter<T>, vec::IntoIter<T>>;
    fn into_iter(self) -> Self::IntoIter {
        let (until_cursor, cursor_and_onwards) = self.inner.split_at(self.cursor);

        let (until_cursor, cursor_and_onwards) =
            (until_cursor.to_vec(), cursor_and_onwards.to_vec());

        cursor_and_onwards.into_iter().chain(until_cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_buffer_contiguous_iter() {
        // Helper-fixture
        macro_rules! assert_indices {
            ($sb:expr, $index_offset:expr) => {
                for (index, elt) in $sb.iter().enumerate() {
                    assert_eq!(index + $index_offset, *elt);
                }
            };
        }
        // Construct with 4 values
        let values: [usize; 4] = core::array::from_fn(|idx| idx);

        let mut sb = Fifo::from_iter(values);

        // Assert indices are matching exactly
        assert_indices!(sb, 0);

        // Overwrite with 4
        // This will now be the last value, and cursor should
        // point to the 1
        sb.overwrite(4);
        // Assert indices + 1 are matching
        assert_indices!(sb, 1);

        // Overwrite with 5
        // This will now be the last value, and cursor should
        // point to the 2
        sb.overwrite(5);
        assert_indices!(sb, 2);
    }
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
}

#[tracing::instrument(level = "debug", skip(dir_entries))]
pub fn dir_entries_to_source_files(
    dir_entries: impl IntoIterator<Item = fs::DirEntry>,
) -> impl Iterator<Item = FileData<Box<[u8]>>> {
    dir_entries
        .into_iter()
        .filter_map(|file| FileData::try_from(file).ok())
}

/// Returns an iterator of lua source-files
#[tracing::instrument(level = "debug", skip(path))]
fn source_files_in_directory<P: AsRef<path::Path> + ?Sized>(
    path: &P,
    // target_extension: &O,
) -> io::Result<impl Iterator<Item = FileData<Box<[u8]>>>> {
    get_files_in_directory_with_extension(path, "lua").map(dir_entries_to_source_files)
}

#[tracing::instrument(level = "debug", skip(source_files))]
pub fn source_files_to_tabs(
    source_files: impl IntoIterator<Item = FileData<Box<[u8]>>>,
) -> impl Iterator<Item = pico_8_cart_model::Tab<'static>> {
    let mut line_number = 0;
    source_files.into_iter().map(move |source_file| {
        tracing::debug!("Currently processing file {:?}", source_file.as_path());
        let file_data = source_file.unwrap_loaded_data_deref();
        let lines_in_file = bytes::NewlineIter::from(file_data).count();
        let section = pico_8_cart_model::Tab {
            line_number,
            code_data: Cow::Owned(source_file.collect_into()),
        };
        line_number += lines_in_file;
        section
    })
}

#[tracing::instrument(level = "debug", skip(tabs))]
pub fn compile_tabs<'a>(
    tabs: impl IntoIterator<Item = pico_8_cart_model::Tab<'a>>,
) -> pico_8_cart_model::CodeTabs<'a> {
    tabs.into_iter()
        .enumerate()
        .fold(Default::default(), |mut tabs, (tab_index, code_tab)| {
            tracing::info!("compiling tab {tab_index}");
            tabs[tab_index] = Some(code_tab);
            tabs
        })
}

#[tracing::instrument(level = "debug", skip(source_files))]
fn compile_source_files_to_tabs(
    source_files: impl IntoIterator<Item = FileData<Box<[u8]>>>,
) -> pico_8_cart_model::CodeTabs<'static> {
    compile_tabs(source_files_to_tabs(source_files))
}

#[tracing::instrument(level = "debug", skip(dir_entries))]
pub fn dir_entries_to_tabs(
    dir_entries: impl IntoIterator<Item = fs::DirEntry>,
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

pub fn get_source_tabs<P: AsRef<path::Path> + ?Sized>(
    src_dir: &P,
) -> io::Result<impl Iterator<Item = pico_8_cart_model::Tab<'static>>> {
    get_lua_files(src_dir).map(dir_entries_to_tabs)
}

pub fn compile_tabs_to_cart_data<'a>(
    tabs: impl IntoIterator<Item = pico_8_cart_model::Tab<'a>>,
) -> pico_8_cart_model::CartData<'a> {
    pico_8_cart_model::CartData::default_with_code_tabs(compile_tabs(tabs))
}

pub trait FromFile {
    fn from_file(file: fs::File) -> io::Result<Self>
    where
        Self: Sized;
}

impl<T> FromFile for Box<T>
where
    T: FromFile,
{
    fn from_file(file: fs::File) -> io::Result<Self>
    where
        Self: Sized,
    {
        tracing::debug!(
            "Loading boxed data {} from file {file:?}",
            core::any::type_name::<T>()
        );
        T::from_file(file).map(Box::from)
    }
}

impl FromFile for Vec<u8> {
    fn from_file(mut file: fs::File) -> io::Result<Self>
    where
        Self: Sized,
    {
        tracing::debug!("Loading byte-vector from file {file:?}");
        let mut buf = vec![];
        io::Read::read_to_end(&mut file, &mut buf)
            .inspect_err(|e| tracing::error!("failed to read source-file to end: {e}"))?;

        Ok(buf)
    }
}

impl FromFile for Box<[u8]> {
    fn from_file(file: fs::File) -> io::Result<Self>
    where
        Self: Sized,
    {
        tracing::debug!("Loading boxed slice from file {file:?}");
        Vec::from_file(file).map(Vec::into_boxed_slice)
    }
}

impl FromFile for pico_8_cart_model::CartData<'static> {
    fn from_file(file: fs::File) -> io::Result<Self>
    where
        Self: Sized,
    {
        tracing::debug!("Loading cart-data from file {file:?}");
        pico_8_cart_model::CartData::from_file(file)
            .inspect_err(|e| tracing::error!("Failed to load cart-data from file: {e}"))
    }
}

#[derive(Clone, Debug)]
pub enum FileData<T> {
    Unloaded(path::PathBuf),
    Loaded { path: path::PathBuf, data: T },
}

impl<T> FileData<T> {
    /// Extracts the cart-data, or panics if the project-file is not loaded
    pub fn unwrap_loaded_data_ref(&self) -> &T {
        match self {
            // StatefulFile::NonExistent => panic!("called `unwrap_loaded_ref` on a non-existent project-file"),
            FileData::Unloaded(_) => {
                panic!("called `unwrap_loaded_data_ref` on an unloaded stateful-file")
            }
            FileData::Loaded { data, .. } => data,
        }
    }
    /// Extracts the cart-data, or panics if the project-file is not loaded
    pub fn unwrap_loaded_data_mut(&mut self) -> &mut T {
        match self {
            // StatefulFile::NonExistent => panic!("called `unwrap_loaded_ref` on a non-existent project-file"),
            FileData::Unloaded(_) => {
                panic!("called `unwrap_loaded_data_ref` on an unloaded stateful-file")
            }
            FileData::Loaded { data, .. } => data,
        }
    }
    /// Extracts the cart-data, or panics if the project-file is not loaded
    ///
    /// uses deref-impl as a helper/optimization
    pub fn unwrap_loaded_data_deref(&self) -> &<T as Deref>::Target
    where
        T: Deref,
    {
        match self {
            // ProjectFile::NonExistent => panic!("called `unwrap_loaded_ref` on a non-existent project-file"),
            FileData::Unloaded(_) => {
                panic!("called `unwrap_loaded_data_deref` on an unloaded stateful-file")
            }
            FileData::Loaded { data, .. } => data,
        }
    }
    /// Extracts the cart-data, or panics if the project-file is not loaded
    pub fn unwrap_loaded_data(self) -> T {
        match self {
            // StatefulFile::NonExistent => panic!("called `unwrap_loaded_ref` on a non-existent project-file"),
            FileData::Unloaded(_) => {
                panic!("called `unwrap_loaded_data_ref` on an unloaded stateful-file")
            }
            FileData::Loaded { data, .. } => data,
        }
    }
    pub fn new<P: AsRef<path::Path> + ?Sized>(file_path: &P) -> FileData<T> {
        FileData::Unloaded(file_path.as_ref().to_path_buf())
    }
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn load(&mut self) -> io::Result<()>
    where
        T: FromFile,
    {
        match self {
            FileData::Unloaded(path) => {
                tracing::debug!("Unloaded at {path:?}");
                let data = fs::OpenOptions::new()
                    .create(true)
                    .read(true)
                    .write(true)
                    .truncate(true)
                    .open(path.as_path())
                    .and_then(T::from_file)?;
                tracing::debug!(
                    "Got data {} of size {}",
                    core::any::type_name_of_val(&data),
                    size_of_val(&data)
                );
                *self = FileData::Loaded {
                    path: path.to_path_buf(),
                    data,
                };
                // .map(|data| StatefulFile::Loaded { path, data })
                Ok(())
            }
            _ => {
                tracing::debug!("Loaded already");
                Ok(())
            }
        }
    }
    #[tracing::instrument(level = "debug", skip(self))]
    pub fn into_loaded(mut self) -> io::Result<FileData<T>>
    where
        T: FromFile,
    {
        tracing::info!("Loading file: {:?}", self.as_path());
        self.load()?;
        Ok(self)
    }

    pub fn as_path(&self) -> &path::Path {
        match self {
            FileData::Unloaded(path) => path,
            FileData::Loaded { path, .. } => path,
        }
        .as_path()
    }

    pub fn has_extension<O: AsRef<ffi::OsStr> + ?Sized>(&self, target_extension: &O) -> bool {
        self.as_path()
            .extension()
            .is_some_and(|extension| extension == target_extension.as_ref())
        // match self {
        //     StatefulFile::Unloaded(path) => path
        //         .extension()
        //         .is_some_and(|extension| extension == target_extension.as_ref()),
        //     StatefulFile::Loaded { path, .. }
        // }
        // dir_entry_has_extension(&self.src_entry, target_extension)
    }
    pub fn is_lua_file(&self) -> bool {
        self.has_extension("lua")
    }
    fn get_name(&self) -> Option<&str> {
        self.as_path().file_stem().and_then(ffi::OsStr::to_str)
    }
    pub const fn is_loaded(&self) -> bool {
        matches!(self, FileData::Loaded { .. })
    }
}
impl<T> FileData<T>
where
    T: AsRef<[u8]> + IntoIterator<Item = u8>,
{
    /// Only to be used for lua source-files
    #[tracing::instrument(level = "debug", skip(self))]
    #[inline(always)]
    fn collect_into<U: FromIterator<u8>>(self) -> U {
        let file_data = self.unwrap_loaded_data_ref();
        if file_data.as_ref().starts_with(b"--") {
            self.unwrap_loaded_data().into_iter().collect()
        } else {
            let name = self
                .get_name()
                .expect("failed to get name for stateful file when collecting");

            // So the pico-8 editor gets a nice title view too, QoL i guess...
            let name_as_comment = format!("-- {name}\n");
            name_as_comment
                .into_bytes()
                .into_iter()
                .chain(self.unwrap_loaded_data())
                .collect()
        }
    }
}
impl<T> TryFrom<fs::DirEntry> for FileData<T> {
    type Error = io::Error;
    #[tracing::instrument(level = "debug")]
    fn try_from(value: fs::DirEntry) -> Result<Self, Self::Error> {
        let path = value.path();

        if path.is_file() {
            tracing::debug!("{path:?} is a file");
            Ok(FileData::Unloaded(path))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{path:?} is not a file"),
            ))
        }
        // let mut file = fs::File::open(value.path())
        //     .inspect_err(|e| tracing::error!("failed to open file as source-file: {e}"))?;

        // let mut buf = vec![];
        // io::Read::read_to_end(&mut file, &mut buf)
        //     .inspect_err(|e| tracing::error!("failed to read source-file to end: {e}"))?;

        // Ok(SourceFile {
        //     src_entry: value,
        //     file_data: buf.into_boxed_slice(),
        // })
    }
}

pub struct SourceFile {
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
    pub fn into_parts(self) -> (fs::DirEntry, Box<[u8]>) {
        let SourceFile {
            src_entry,
            file_data,
        } = self;
        (src_entry, file_data)
    }
}

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
    pub fn collect_into<T: FromIterator<u8>>(self) -> T {
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

/// Takes an iterator over files selected to
/// be compiled, and the output cart-path
///
/// Attempts to merge together the code sections
///
/// TODO: Proper merge-logic
pub fn compile_cartridge(
    cart_file: FileData<Box<pico_8_cart_model::CartData<'static>>>,
    source_files: impl Iterator<Item = FileData<Box<[u8]>>>,
) -> io::Result<pico_8_cart_model::CartData<'static>> {
    // construct the tabs
    let tabs = source_files_to_tabs(source_files);

    // Compile the code-tabs
    let code_tabs: pico_8_cart_model::CodeTabs =
        tabs.enumerate()
            .fold(Default::default(), |mut tabs, (tab_index, code_tab)| {
                tracing::info!("compiling tab {tab_index}");
                tabs[tab_index] = Some(code_tab);
                tabs
            });

    let code_tab_count = code_tabs.iter().filter(|elt| elt.is_some()).count();
    tracing::info!("Compiling {code_tab_count} tabs");

    // We don't want to ignore the content of the pico-8 `main`
    // file and overwrite it without modifying it first
    //
    // TODO: Merge code-sections (might be really really complicated)
    // let mut cart = pico_8_cart_model::CartData::from_path_or_default(cart_file)?;

    let mut cart = *cart_file.unwrap_loaded_data();

    // Overwrite the cart-data and recopy it
    if code_tabs.iter().any(Option::is_some) {
        cart.set_code_data(code_tabs);
    }
    Ok(cart)
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
#[tracing::instrument(level = "debug", skip(cart_src), ret)]
fn get_section_delimiters(
    cart_src: &[u8],
    line_number_offset: Option<usize>,
) -> Vec<SectionDelimiter<'static>> {
    tracing::debug!(
        "getting section delimiters from cart_src.len()={}",
        cart_src.len()
    );
    let mut offset = 0;
    // add 1 to compensate non-zero start of file
    let line_number_offset = line_number_offset.unwrap_or_default() + 1;
    let mut section_delimiters: Vec<SectionDelimiter<'static>> = bytes::NewlineIter::new(cart_src)
        .enumerate()
        .filter_map(|(line_number, line)| {
            let line_number_with_offset = line_number + line_number_offset;
            let delimiter = section::get_line_type(line).map(|ty| {
                tracing::debug!(
                    "Section of {ty:?} starts at {line_number_with_offset}: {:?}",
                    core::str::from_utf8(line)
                );
                SectionDelimiter {
                    ty,
                    line_number: line_number_with_offset,
                    offset,
                }
            });
            if delimiter.is_none() {
                tracing::debug!(
                    "{line_number_with_offset}: {:?}",
                    core::str::from_utf8(line)
                );
            }
            offset += line.len();
            delimiter
        })
        .collect();
    // Sort, so that sections are in order of line-number
    section_delimiters.sort();
    section_delimiters
}

/// Returns the section-delimiters ordered by line-number
#[tracing::instrument(skip(cart_src), ret)]
fn get_section_delimiters_v2(
    cart_src: &[u8],
    line_number_offset: Option<usize>,
) -> impl Iterator<Item = pico_8_cart_model::SectionDelimiter> {
    let mut byte_offset = 0;
    bytes::NewlineIter::new(cart_src)
        .enumerate()
        .filter_map(move |(line_number, line)| {
            let line_number_with_offset =
                line_number + (line_number_offset.unwrap_or_default() + 1);
            let delimiter = pico_8_cart_model::section::get_line_type(line)
                .copied()
                .map(|r#type| {
                    tracing::debug!(
                        "Section of {type:?} starts at {line_number_with_offset}: {:?}",
                        core::str::from_utf8(line)
                    );
                    pico_8_cart_model::SectionDelimiter {
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
fn get_sections(
    cart_src: &[u8],
    delimiters: impl Iterator<Item = pico_8_cart_model::SectionDelimiter>,
) -> impl Iterator<Item = pico_8_cart_model::Section<'_>> {
    // Collect so that we may sort
    let mut sorted_delimiters: Vec<pico_8_cart_model::SectionDelimiter> = delimiters.collect();

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
                pico_8_cart_model::SectionDelimiter {
                    r#type,
                    line_number,
                    byte_offset,
                },
            )| {
                let type_string =
                    <&'static str as From<&pico_8_cart_model::SectionType>>::from(&r#type);
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

                let section = pico_8_cart_model::Section::new(r#type, line_number, section_src);

                tracing::debug!(
                    "[Line: {line_number:>4} | Size: {:>6} | Offset: {offset_without_type_marker:>6} -> {:>6}] {type:?}",
                    section_src.len(),
                    offset_without_type_marker + section_src.len()
                );
                Some(section)
            },
        )
}

impl<'a> P8CartData<'a> {
    #[tracing::instrument(level = "debug", skip(cart_src))]
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
                let offset_without_type_marker = offset + ty_str.len() + 1;
                let section_src = if idx == 0 {
                    cart_src.get(offset_without_type_marker..)
                } else {
                    cart_src.get(offset_without_type_marker..next_section_offset)
                }?;
                next_section_offset = offset;

                let section_data: &SectionData = unsafe { transmute(section_src) };
                let section = Section {
                    ty,
                    line_number,
                    data: Cow::Borrowed(section_data),
                };
                tracing::debug!(
                    "[Line: {line_number:>4} | Size: {:>6} | Offset: {offset_without_type_marker:>6} -> {:>6}] {ty:?}",
                    section_src.len(),
                    offset_without_type_marker + section_src.len()
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

const P8_MAX_CODE_EDITOR_TAB_COUNT: usize = 16;

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
            .field("code.len()", &self.code.0.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct P8CodeData<'a> {
    tabs: [Option<Tab<'a>>; P8_MAX_CODE_EDITOR_TAB_COUNT],
}

impl<'a> P8CodeData<'a> {
    #[tracing::instrument(level = "debug", skip(section_data), ret)]
    fn from_lua_section(mut line_number: usize, section_data: &'a SectionData) -> P8CodeData<'a> {
        let mut tabs = <[Option<Tab<'a>>; P8_MAX_CODE_EDITOR_TAB_COUNT]>::default();
        let tab_iter = bytes::TabIter::new(&section_data.0);

        // Increment over the __lua__ marker
        line_number += 1;

        for (idx, tab) in tab_iter.enumerate() {
            // Increment over previous iteration tab-separator (not for first)
            if idx != 0 {
                line_number += 1;
            };
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
    header: Cow<'a, pico_8_cart_model::Header>,
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

// #[cfg(test)]
// mod test_data {
//     /// version 43
//     ///
//     /// empty other than gfx
//     pub(crate) const ONLY_GFX_SECTION_MAX_TABS: &str = r"pico-8 cartridge // http://www.pico-8.com
// version 43
// __lua__
// -- tab 0
// -->8
// -- tab 1
// -->8
// -- tab 2
// -->8
// -- tab 3
// -->8
// -- tab 4
// -->8
// -- tab 5
// -->8
// -- tab 6
// -->8
// -- tab 7
// -->8
// -- tab 8
// -->8
// -- tab 9
// -->8
// -- tab a
// -->8
// -- tab b
// -->8
// -- tab c
// -->8
// -- tab d
// -->8
// -- tab e
// -->8
// -- tab f
// __gfx__
// 00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
// 00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
// 00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
// 00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
// 00077000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
// 00700700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000
// ";
// }
