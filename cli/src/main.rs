use std::fs;

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
    tracing::info!("Opening cart at {cart_path:?}");

    let mut cart_file = fs::File::open(cart_path)?;

    let cart = pico_build_rs::P8Cart::try_from_reader(&mut cart_file)
        .map_err(|e| anyhow!("Failed to read cart data from file: {e}"));
    tracing::info!("{cart:#?}");

    Ok(())
}
