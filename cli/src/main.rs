use std::fs;
use std::io;

use anyhow::anyhow;
use clap::Parser;

mod args;
mod config;

#[tracing::instrument(level = "info", ret)]
fn main() -> anyhow::Result<()> {
    use crate::args::AppArgs;
    use crate::config::AppConfiguration;

    tracing_subscriber::fmt()
        .with_max_level(tracing::level_filters::STATIC_MAX_LEVEL)
        .init();

    let args = AppArgs::parse();

    let cfg = AppConfiguration::new(&args)?;
    tracing::info!("{cfg:#?}");

    let cart_path = cfg
        .cart_path()
        .ok_or_else(|| anyhow!("Configuration invalid, cart path does not point to a file"))?;
    // tracing::info!("Opening cart at {cart_path:?}");

    // let mut cart_file = fs::File::open(cart_path)?;

    // // Buffer for file-data
    // let mut cart_src = vec![];

    // // Copy file-data
    // io::Read::read_to_end(&mut cart_file, &mut cart_src)?;

    // let cart = pico_8_cart_model::CartData::from_cart_source(cart_src.as_ref())?;

    // let cart = pico_build_rs::P8Cart::try_from_reader(&mut cart_file)
    //     .map_err(|e| anyhow!("Failed to read cart data from file: {e}"));

    let cart = pico_8_cart_builder::CartBuilder::new(cfg.src_dir).build(&cart_path)?;
    tracing::info!("{cart:#?}");

    let mut file = fs::OpenOptions::new()
        .write(true)
        .read(true)
        .append(false)
        .truncate(true)
        .create(true)
        .open("main_dst.p8")?;
    let cart_src: Box<[u8]> = cart.into_cart_source();
    io::Write::write_all(&mut file, cart_src.as_ref())?;

    let original_file = fs::read_to_string(&cart_path)?;
    let copied_file = fs::read_to_string("main_dst.p8")?;

    if original_file == copied_file {
        tracing::info!("The files match!")
    } else {
        tracing::warn!("The files were different...")
    }

    Ok(())
}
