use anyhow::anyhow;

use std::path;

use crate::args::AppArgs;

// /// Either uses the configuration file provided
// /// as cli-arguments, or attempts to resolve from
// /// environment.
// ///
// /// The borrowed value comes from the provided [`AppArgs`],
// /// the owned one being a [`PathBuf`](path::PathBuf) from
// /// the environment-variable (if found).
// fn get_config_file_path(
//     args: &AppArgs
// ) -> Option<Cow<'_, path::Path>> {

//         .as_deref()
//         .map(Cow::Borrowed)
//         .or(option_env!("PICO_BUILD_CONFIG_FILE_PATH")
//             .map(path::PathBuf::from)
//             .map(Cow::Owned))
// }

pub fn try_from_path<P: AsRef<path::Path> + ?Sized>(
    config_file_path: &P,
) -> Result<config::Config, config::ConfigError> {
    let config_file_path: &path::Path = config_file_path.as_ref();
    let config_file: config::File<config::FileSourceFile, config::FileFormat> =
        config::File::from(config_file_path);
    config::Config::builder().add_source(config_file).build()
}

pub struct AppConfigFile {
    values: config::Config,
}

impl TryFrom<&path::Path> for AppConfigFile {
    type Error = config::ConfigError;
    fn try_from(config_file_path: &path::Path) -> Result<Self, Self::Error> {
        try_from_path(config_file_path).map(AppConfigFile::from)
    }
}

impl From<config::Config> for AppConfigFile {
    fn from(value: config::Config) -> Self {
        AppConfigFile { values: value }
    }
}

impl AppConfigFile {
    pub fn open(args: &AppArgs) -> anyhow::Result<AppConfigFile> {
        let root_dir = args.get_root_directory()?;
        let toml_path = root_dir.join("pico.toml");
        let json_path = root_dir.join("pico.json");
        let mut config_file = None;

        if toml_path.exists() {
            config_file = AppConfigFile::try_from(toml_path.as_path()).map(Some)?
        } else if json_path.exists() {
            config_file = AppConfigFile::try_from(json_path.as_path()).map(Some)?
        };

        config_file.ok_or_else(|| anyhow!("No config file found."))
    }
}

/// The set of values defining
/// runtime-behavior for the `pico-build-rs`
/// command-line interface
#[derive(Debug)]
pub struct AppConfiguration {
    /// Required.
    ///
    /// The source-directory for pico-8 lua files
    pub src_dir: path::PathBuf,
    /// Required.
    ///
    /// The name of the cart-project
    pub cart: String,
    /// Required. (but if not found, false will be used)
    ///
    /// Whether to automatically rebuild
    /// once an update has been seen on one of
    /// the source-files.
    ///
    /// unimplemented atm.
    pub watch: bool,
    /// Required. (but if not found, false will be used)
    ///
    /// Whether to open up the pico-8 executable.
    pub open_pico: bool,
    /// Not required
    ///
    /// Application will return an error
    /// if `open_pico` has been set to true,
    /// while no executable path has been provided.
    pub executable: Option<path::PathBuf>,
}
impl AppConfiguration {
    pub fn new(args: &AppArgs) -> anyhow::Result<AppConfiguration> {
        if let Some((src_dir, cart, watch, open_pico, executable)) = args.configuration_values() {
            // In this case we assume the user explicitly intended this,
            // due to how cumbersome it would be to type all the args out fully
            Ok(AppConfiguration {
                src_dir: src_dir.to_path_buf(),
                cart: cart.into(),
                watch,
                open_pico,
                executable: executable.map(path::Path::to_path_buf),
            })
        } else {
            let root_dir = args.get_root_directory()?;
            let toml_path = root_dir.join("pico.toml");
            let json_path = root_dir.join("pico.json");
            let mut config_file = None;

            if toml_path.exists() {
                config_file = AppConfigFile::try_from(toml_path.as_path()).map(Some)?
            } else if json_path.exists() {
                config_file = AppConfigFile::try_from(json_path.as_path()).map(Some)?
            };

            let Some(config_file) = config_file else {
                return Err(anyhow!("No config file found."));
            };

            let src_dir = match config_file.values.get_string("src_dir") {
                Ok(val) => path::PathBuf::from(val),
                Err(e) => {
                    if let Some(src_dir_arg) = args.get_src_dir() {
                        src_dir_arg.to_path_buf()
                    } else {
                        return Err(anyhow!(
                            "Failed to find source-dir configuration value in args, or in config-file due to {e}"
                        ));
                    }
                }
            };

            let cart = match config_file.values.get_string("cart") {
                Ok(val) => val,
                Err(e) => {
                    if let Some(cart_arg) = args.get_cart() {
                        cart_arg.into()
                    } else {
                        return Err(anyhow!(
                            "Failed to find source-dir configuration value in args, or in config-file due to {e}"
                        ));
                    }
                }
            };

            let watch = config_file.values.get_bool("watch").unwrap_or(args.watch);
            let open_pico = config_file
                .values
                .get_bool("open_pico")
                .unwrap_or(args.open_pico);

            let executable = config_file
                .values
                .get_string("executable")
                .map(path::PathBuf::from)
                .ok()
                .or_else(|| args.get_executable().map(path::Path::to_path_buf));

            Ok(AppConfiguration {
                src_dir,
                cart,
                watch,
                open_pico,
                executable,
            })
        }
    }
    pub fn cart_path(&self) -> Option<path::PathBuf> {
        let mut cart_path = self.src_dir.clone();
        cart_path.push(self.cart.as_str());

        if cart_path.exists() {
            Some(cart_path)
        } else {
            None
        }
    }
}
