use std::fs;
use std::path;

pub struct Pico8CartBuilder {
    src_dir: path::PathBuf,
}

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

impl Pico8CartBuilder {
    fn file_builders(&self) -> std::io::Result<impl Iterator<Item = FileBuilder>> {
        let path = self.src_dir.as_path();
        get_lua_files(path).map(|lua_files| lua_files.map(Into::into))
    }
}

struct FileBuilder {
    src_entry: fs::DirEntry,
}
impl From<fs::DirEntry> for FileBuilder {
    fn from(value: fs::DirEntry) -> Self {
        FileBuilder { src_entry: value }
    }
}
impl FileBuilder {
    fn get_name(&self) -> String {
        self.src_entry
            .file_name()
            .into_string()
            .expect("pico-8 source file name contained invalid utf-8")
    }
}
