use std::borrow::Cow;
use std::path;

use clap::Parser;

#[expect(dead_code)] // here as a detail on the long_about
const CRAB_EMOJI: char = '\u{1f980}';

#[expect(dead_code)] // here as a detail on the long_about
const GEAR_EMOJI: char = '\u{2699}';

#[derive(Debug, Parser)]
#[command(
    version,
    about,
    long_about = "pico-build rewritten in rust \u{1f980}\u{2699}"
)]
pub struct AppArgs {
    /// The root-directory to use for
    /// the pico-build-rs command-line interface.
    ///
    /// If not set here, the environment variable
    /// `PICO_BUILD_ROOT_DIRECTORY` will be used
    /// instead.
    ///
    /// If neither are used, the working-directory will be assumed.
    #[arg(short = 'c', long = "config", value_name = "CONFIG_FILE_PATH")]
    root_directory: Option<path::PathBuf>,

    /// The directory to be used for pico-8 source-files
    #[arg(short, long, value_name = "SRC_DIR")]
    src_dir: Option<path::PathBuf>,
    /// The cart-name
    #[arg(long, value_name = "CART")]
    cart: Option<String>,
    /// Whether to automatically update on changes to the lua
    #[arg(short, long, value_name = "WATCH", default_value_t = false)]
    pub watch: bool,
    /// Open pico executable i guess or something TODO: figure this out
    #[arg(short, long, value_name = "OPEN_PICO", default_value_t = false)]
    pub open_pico: bool,
    /// The executable-path to be used if `open_pico` is specified
    #[arg(short, long, value_name = "EXECUTABLE", default_value = "None")]
    executable: Option<path::PathBuf>,
}
impl AppArgs {
    pub fn get_root_directory(&self) -> std::io::Result<Cow<'_, path::Path>> {
        if let Some(dir) = self.root_directory.as_deref() {
            return Ok(Cow::Borrowed(dir));
        };

        std::env::current_dir().map(Cow::Owned)
    }
    /// Returns `true` if the state of arguments are such that
    /// the runtime can be entirely configured from it alone
    pub fn can_become_config(&self) -> bool {
        let required_values_set = self.src_dir.is_some() && self.cart.is_some();
        let valid_executable_state = if self.open_pico {
            self.executable.is_some()
        } else {
            true
        };
        required_values_set && valid_executable_state
    }
    pub fn get_src_dir(&self) -> Option<&path::Path> {
        self.src_dir.as_deref()
    }
    pub fn get_cart(&self) -> Option<&str> {
        self.cart.as_deref()
    }
    pub fn get_executable(&self) -> Option<&path::Path> {
        self.executable.as_deref()
    }
    pub fn configuration_values(
        &self,
    ) -> Option<(&path::Path, &str, bool, bool, Option<&path::Path>)> {
        let AppArgs {
            watch,
            open_pico,
            executable,
            ..
        } = self;

        // Invalid state for running the executable
        if *open_pico && executable.is_none() {
            return None;
        };

        // let root_directory = root_directory.as_deref()?;
        // let src_dir = src_dir.as_deref()?;
        // let cart = cart.as_deref()?;
        Some((
            self.get_src_dir()?,
            self.get_cart()?,
            *watch,
            *open_pico,
            self.get_executable(),
        ))
    }
}
