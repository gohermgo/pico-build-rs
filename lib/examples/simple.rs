use std::fs;
use std::path;
use std::process::ExitCode;

use tracing_subscriber::filter::LevelFilter;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    let Some(max_tabs_cart) = read_cartridge_file("maxtabs.p8") else {
        return ExitCode::FAILURE;
    };

    tracing::info!("Successfully read cart: {max_tabs_cart:#?}");

    let Some(main_cart) = read_cartridge_file("main.p8") else {
        return ExitCode::FAILURE;
    };

    tracing::info!("Successfully read cart: {main_cart:#?}");

    ExitCode::SUCCESS
}

fn get_cartridge_main_file_path<P: AsRef<path::Path>>(file_name: P) -> Option<path::PathBuf> {
    option_env!("CARGO_MANIFEST_DIR")
        .map(path::PathBuf::from)
        .map(|mut path| {
            // We are in workspace, src is in root
            path.pop();
            path.push("pico-build-test-src");
            path.push(file_name);
            path
        })
}

fn open_cartridge_file<P: AsRef<path::Path>>(file_name: P) -> Option<fs::File> {
    let Some(cartridge_file_path) = get_cartridge_main_file_path(file_name) else {
        tracing::warn!("Failed to find cargo manifest-directory in environment");
        return None;
    };
    fs::File::open(cartridge_file_path)
        .inspect_err(|e| tracing::warn!("Failed to open p8-file: {e}"))
        .ok()
}

fn read_cartridge_file<P: AsRef<path::Path>>(
    file_name: P,
) -> Option<pico_8_cart_model::CartData<'static>> {
    open_cartridge_file(file_name).and_then(|cartridge_file| {
        pico_8_cart_model::CartData::from_file(cartridge_file)
            .inspect_err(|e| tracing::warn!("Failed to read cartridge file: {e}"))
            .ok()
    })
}
