use std::fs;
use std::path;
use std::process::ExitCode;

use tracing_subscriber::filter::LevelFilter;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    let Some(cartridge_main_file) = get_cartridge_main_file_path("maxtabs.p8") else {
        tracing::warn!("Failed to find cargo manifest-directory in environment");
        return ExitCode::FAILURE;
    };

    let mut file = match fs::File::open(cartridge_main_file) {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!("Failed to open p8-file: {e}");
            return ExitCode::FAILURE;
        }
    };

    let cart = match pico_build_rs::P8Cart::try_from_reader(&mut file) {
        Ok(cart) => cart,
        Err(e) => {
            tracing::warn!("Failed to read cart: {e}");
            return ExitCode::FAILURE;
        }
    };

    tracing::info!("Successfully read cart: {cart:#?}");
    // let mut file = std::fs::File::open()
    ExitCode::SUCCESS
}

fn get_cartridge_main_file_path<P: AsRef<path::Path>>(file_name: P) -> Option<path::PathBuf> {
    option_env!("CARGO_MANIFEST_DIR")
        .map(path::PathBuf::from)
        .map(|mut path| {
            path.push(file_name);
            path
        })
}
