use std::fs;
use std::io;
use std::path;

fn get_pico_8_cart<P: AsRef<path::Path> + ?Sized>(path: &P) -> io::Result<fs::File> {
    todo!()
}
/// Attempts to parse an empty cartridge
#[test]
fn empty() {}
