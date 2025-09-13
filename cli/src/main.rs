use core::ops::Deref;
use core::time::Duration;

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path;
use std::sync::mpsc;

use anyhow::anyhow;
use clap::Parser;
use ratatui::prelude::*;

mod args;
mod config;
mod log_panel;

use log_panel::LogPanelWidget;

use pico_build_rs::StatefulFile;

/// The size of the log-panel in log-lines
const LOG_LINE_COUNT: usize = 20;

enum ProjectFile {
    // NonExistent,
    Unloaded(path::PathBuf),
    Loaded {
        path: path::PathBuf,
        data: pico_8_cart_model::CartData<'static>,
    },
}

impl ProjectFile {
    /// Extracts the cart-data, or panics if the project-file is not loaded
    pub fn unwrap_loaded_data_ref(&self) -> &pico_8_cart_model::CartData<'static> {
        match self {
            // ProjectFile::NonExistent => panic!("called `unwrap_loaded_ref` on a non-existent project-file"),
            ProjectFile::Unloaded(_) => {
                panic!("called `unwrap_loaded_ref` on an unloaded project-file")
            }
            ProjectFile::Loaded { data, .. } => data,
        }
    }
    pub fn new<P: AsRef<path::Path> + ?Sized>(file_path: &P) -> ProjectFile {
        ProjectFile::Unloaded(file_path.as_ref().to_path_buf())
    }
    pub fn load(&mut self) -> io::Result<()> {
        match self {
            ProjectFile::Unloaded(path) => {
                let data = fs::OpenOptions::new()
                    .create(true)
                    .read(true)
                    .write(true)
                    .truncate(true)
                    .open(path.as_path())
                    .and_then(pico_8_cart_model::CartData::from_file)?;
                *self = ProjectFile::Loaded {
                    path: path.to_path_buf(),
                    data,
                };
                // .map(|data| ProjectFile::Loaded { path, data })
                Ok(())
            }
            _ => Ok(()),
        }
    }
    pub fn into_loaded(mut self) -> io::Result<ProjectFile> {
        self.load()?;
        Ok(self)
    }
}

struct ModelV2 {
    project_file: StatefulFile<Box<pico_8_cart_model::CartData<'static>>>,
    source_directory: path::PathBuf,
    source_files: Box<[StatefulFile<Box<[u8]>>]>,
    running_state: RunningState,
}

impl ModelV2 {
    fn new(cfg: config::AppConfiguration) -> io::Result<ModelV2> {
        let project_file = StatefulFile::new(&cfg.cart_path());

        let mut app = ModelV2 {
            project_file,
            source_directory: cfg.src_dir,
            source_files: Box::default(),
            running_state: RunningState::Running,
        };

        Ok(app)
    }

    /// Discovers all source files in the configured directory
    fn discover_source_files(&self) -> io::Result<impl Iterator<Item = StatefulFile<Box<[u8]>>>> {
        pico_build_rs::get_lua_files(self.source_directory.as_path())
            .map(pico_build_rs::dir_entries_to_source_files)
    }

    /// Reads the stateful files into memory
    fn read_source_files(&mut self) -> io::Result<()> {
        for source_file in self.source_files.iter_mut() {
            source_file.load()?;
        }

        Ok(())
    }

    /// Loads all source files in the configured directory
    fn load_source_files(&mut self) -> io::Result<()> {
        let source_files = self.discover_source_files().map(Box::from_iter)?;

        self.source_files = source_files;

        self.read_source_files()
    }

    /// Creates or loads the project-file
    fn load_project_file(&mut self) -> io::Result<()> {
        self.project_file.load()
    }

    /// Compile a new cartridge based on internal state
    ///
    /// Expects that [`ModelV2::load_source_files`] and [`ModelV2::load_project_file`] has been
    /// called previously. Otherwise `Err` will be returned.
    fn compile_cartridge(&self) -> io::Result<pico_8_cart_model::CartData<'static>> {
        if !self.project_file.is_loaded() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Project file not loaded",
            ));
        }

        if !self.source_files.iter().all(StatefulFile::is_loaded) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Not all source-files are loaded",
            ));
        }

        pico_build_rs::compile_cartridge(
            self.project_file.clone(),
            self.source_files.iter().cloned(),
        )
    }
}

#[tracing::instrument(level = "info", ret)]
fn main() -> anyhow::Result<()> {
    use crate::args::AppArgs;
    use crate::config::AppConfiguration;

    let message_rx = log_panel::setup_tracing_subscriber();

    let args = AppArgs::parse();

    let cfg = AppConfiguration::new(&args)?;
    tracing::info!("parsed app configuration");
    tracing::trace!("{cfg:#?}");
    tracing::info!("source directory is {:?}", cfg.src_dir);
    let cart_path = cfg.cart_path();
    tracing::info!("cart path is {:?}", cart_path);
    let mut terminal = ratatui::init();
    let log_lines: [Line<'static>; LOG_LINE_COUNT] = core::array::from_fn(|_| Line::default());
    let log_messages = pico_build_rs::ScrollBuffer::from(Box::from(log_lines));
    let log_panel_chunk = get_rect(&mut terminal.get_frame(), log_messages.len())[0];
    let mut model = Model {
        src_dir: cfg.src_dir.clone(),
        cart_path,
        log_message_rx: message_rx,
        log_messages,
        log_message_capacity: log_panel_chunk.height as usize,
        running_state: RunningState::Running,
        file_loading_tracker: FileLoadingTracker {
            paths: Default::default(),
        },
    };
    while !matches!(model.running_state, RunningState::Done) {
        terminal.draw(|frame| view(&model, frame))?;

        let mut current_message = handle_event(&model);

        while current_message.is_some() {
            current_message = update(&mut model, current_message.unwrap());
            if matches!(current_message.as_ref(), Some(&Message::ClearLog)) {
                // terminal.swap_buffers();
                // let rect = get_rect(&mut terminal.get_frame())[1];
                // let mut frame = terminal.get_frame();
                // let buffer = frame.buffer_mut();
                // for position in rect.positions() {
                //     buffer[position].set_symbol("JUNK");
                // }
                // terminal.swap_buffers();
                terminal.clear()?;
            }
        }
    }
    // loop {
    //     terminal.draw(|frame| {
    //         let chunks = Layout::default()
    //             .direction(Direction::Vertical)
    //             .constraints([
    //                 Constraint::Min(1),         // big main-box
    //                 Constraint::Percentage(50), // log-box
    //             ])
    //             .split(frame.area());
    //         frame.render_widget(
    //             widgets::Block::new()
    //                 .title("main")
    //                 .borders(widgets::Borders::ALL),
    //             chunks[0],
    //         );

    //         if let Ok(msg) = message_rx.try_recv() {
    //             log_state.push(msg);
    //         }
    //         frame.render_stateful_widget(LogWidget {}, chunks[1], &mut log_state);
    //     })?;
    //     if event::poll(Duration::from_millis(10))? {
    //         match event::read()? {
    //             event::Event::Key(key) if key.kind == event::KeyEventKind::Press => {
    //                 match key.code {
    //                     event::KeyCode::Char('q') => break,
    //                     event::KeyCode::Enter => {
    //                         let cart = pico_8_cart_builder::CartBuilder::new(&cfg.src_dir)
    //                             .build(&cart_path)?;
    //                         tracing::info!("got cart");
    //                         tracing::trace!("{cart:#?}")
    //                     }
    //                     _ => {}
    //                 }
    //             }
    //             _ => {}
    //         }
    //     }
    // }
    ratatui::restore();
    Ok(())
    // tracing::info!("Opening cart at {cart_path:?}");

    // let mut cart_file = fs::File::open(cart_path)?;

    // // Buffer for file-data
    // let mut cart_src = vec![];

    // // Copy file-data
    // io::Read::read_to_end(&mut cart_file, &mut cart_src)?;

    // let cart = pico_8_cart_model::CartData::from_cart_source(cart_src.as_ref())?;

    // let cart = pico_build_rs::P8Cart::try_from_reader(&mut cart_file)
    //     .map_err(|e| anyhow!("Failed to read cart data from file: {e}"));

    // let cart = pico_8_cart_builder::CartBuilder::new(cfg.src_dir).build(&cart_path)?;
    // tracing::info!("{cart:#?}");

    // let mut file = fs::OpenOptions::new()
    //     .write(true)
    //     .read(true)
    //     .append(false)
    //     .truncate(true)
    //     .create(true)
    //     .open("main_dst.p8")?;
    // let cart_src: Box<[u8]> = cart.into_cart_source();
    // io::Write::write_all(&mut file, cart_src.as_ref())?;

    // let original_file = fs::read_to_string(&cart_path)?;
    // let copied_file = fs::read_to_string("main_dst.p8")?;

    // if original_file == copied_file {
    //     tracing::info!("The files match!")
    // } else {
    //     tracing::warn!("The files were different...")
    // }
}

use crossterm::event;
use crossterm::event::{Event, KeyCode, KeyEvent};

fn layout(log_panel_lines: usize) -> Layout {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),                         // big main-box
            Constraint::Length(log_panel_lines as u16), // log-box
        ])
}
fn get_rect(frame: &mut Frame, log_panel_lines: usize) -> std::rc::Rc<[Rect]> {
    layout(log_panel_lines).split(frame.area())
}
/// Subject to change (might want to use [`Line`] instead)
type LogLine = Line<'static>;

#[derive(Debug)]
enum FileLoadingState {
    Opened(path::PathBuf),
}
struct FileLoadingTracker {
    paths: HashMap<String, FileLoadingState>,
}
struct Model {
    src_dir: path::PathBuf,
    cart_path: path::PathBuf,
    /// Checked during the [`handle_event`] call
    log_message_rx: mpsc::Receiver<String>,

    /// An owned log-message
    log_messages: pico_build_rs::ScrollBuffer<Line<'static>>,
    /// The implied capacity for log-messages
    log_message_capacity: usize,

    running_state: RunningState,
    file_loading_tracker: FileLoadingTracker,
}
enum RunningState {
    Done,
    Running,
}
/// A statement about how the model should change
enum Message {
    // /// A request to open
    // OpenCartridge {
    //     cartridge_directory_path: path::PathBuf,
    // },
    /// A request to compile the cartridge was made
    CompileCartridge {
        src_dir: path::PathBuf,
        cart_path: path::PathBuf,
    },

    /// The compilation-request finished successfully
    CompilationOutput {
        compiled_data: Box<pico_8_cart_model::CartData<'static>>,
        cart_path: path::PathBuf,
    },

    /// A request to quit was made
    Quit,

    /// A log-line has arrived
    IncomingLogLine(LogLine),

    ClearLog,
}

// struct Message {
//     /// The log needs an update, and the terminal might need a forced redraw
//     incoming_log_message: Option<String>,
//     /// The user has made a request, some action needs to be taken
//     user_request: Option<UserRequest>,
// }

/// Make a decision regarding how the model should change
fn handle_event(model @ Model { log_message_rx, .. }: &Model) -> Option<Message> {
    if let Ok(log_message) = log_message_rx.try_recv() {
        return Some(Message::IncomingLogLine(log_message.into()));
    };

    match event::poll(Duration::from_millis(10)) {
        Err(_) | Ok(false) => None,
        Ok(true) => {
            if let Ok(Event::Key(key)) = event::read() {
                handle_key(model, key)
            } else {
                None
            }
        }
    }
}
fn handle_key(
    Model {
        src_dir, cart_path, ..
    }: &Model,
    key: KeyEvent,
) -> Option<Message> {
    match key.code {
        KeyCode::Enter if key.is_press() => Some(Message::CompileCartridge {
            src_dir: src_dir.clone(),
            cart_path: cart_path.clone(),
        }),
        KeyCode::Char('q' | 'Q') => Some(Message::Quit),
        KeyCode::Char('c' | 'C') => Some(Message::ClearLog),
        _ => None,
    }
}
fn update(
    Model {
        log_messages,
        log_message_capacity,
        running_state,
        file_loading_tracker,
        ..
    }: &mut Model,
    message: Message,
) -> Option<Message> {
    // if let Some(log_message) = incoming_log_message {
    //     while log_messages.len() >= *log_message_capacity {
    //         log_messages.pop_front();
    //     }
    //     log_messages.push_back(log_message.into());
    // }

    match message {
        Message::ClearLog => {
            for message in log_messages.iter_mut() {
                *message = Line::default();
            }
            log_messages.reset_cursor();
            None
        }

        // Message::OpenCartridge {
        //     cartridge_directory_path,
        // } => {
        //     let cartridge_name = cartridge_directory_path
        //         .file_name()
        //         .map(|file_name| file_name.to_string_lossy().into_owned())
        //         .expect("cartridge_name");
        //     file_loading_tracker.paths.insert(
        //         cartridge_name,
        //         FileLoadingState::Opening(cartridge_directory_path),
        //     );
        // }
        Message::CompileCartridge { src_dir, cart_path } => {
            struct FileLoadingWidget {
                path: path::PathBuf,
                status: bool,
            }

            tracing::info!("Writing to cart-path {cart_path:?}");
            let source_files = match pico_build_rs::get_lua_files(src_dir.as_path()) {
                Ok(files) => files.inspect(|entry| {
                    let path = entry.path();
                    let name = path
                        .file_name()
                        .map(|file_name| file_name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    file_loading_tracker
                        .paths
                        .insert(name, FileLoadingState::Opened(path));
                }),
                Err(e) => {
                    tracing::error!("Failed to get lua files {e}");
                    return None;
                }
            }
            .filter_map(|source_entry| {
                StatefulFile::try_from(source_entry)
                    .inspect_err(|e| tracing::error!("Failed to convert source-entry: {e}"))
                    .and_then(StatefulFile::into_loaded)
                    .ok()
            });
            match StatefulFile::new(cart_path.as_path())
                .into_loaded()
                .and_then(|cart_file| pico_build_rs::compile_cartridge(cart_file, source_files))
            {
                Ok(cart) => Some(Message::CompilationOutput {
                    compiled_data: Box::new(cart),
                    cart_path,
                }),
                Err(e) => {
                    tracing::error!("Failed to compile {e}");
                    None
                }
            }
            // match pico_build_rs::compile_cartridge(cart_path.as_path(), source_files) {
            //     Ok(cart) => Some(Message::CompilationOutput {
            //         compiled_data: Box::new(cart),
            //         cart_path,
            //     }),
            //     Err(e) => {
            //         tracing::error!("Failed to compile {e}");
            //         None
            //     }
            // }
            // let mut lua_files: Box<[fs::DirEntry]> = pico_8_cart_builder::get_lua_files(&src_dir)
            //     .map(|entries| {
            //         entries
            //             .inspect(|entry| {
            //                 let path = entry.path();
            //                 let name = path
            //                     .file_name()
            //                     .map(|file_name| file_name.to_string_lossy().into_owned())
            //                     .unwrap_or_default();
            //                 file_loading_tracker
            //                     .paths
            //                     .insert(name, FileLoadingState::Opened(path));
            //             })
            //             .collect()
            //     })
            //     .inspect_err(|e| tracing::error!(" failed to get lua files {e}"))
            //     .ok()?;
            // lua_files.sort_by_key(|dir_entry| dir_entry.path());
            // let tabs = pico_8_cart_builder::dir_entries_to_tabs(lua_files.into_iter());
            // match pico_8_cart_builder::merge_tabs_with_src(&cart_path, tabs) {
            //     Ok(cart) => Some(Message::CompilationOutput {
            //         compiled_data: Box::new(cart),
            //         cart_path,
            //     }),
            //     Err(e) => {
            //         tracing::error!("{e}");
            //         tracing::error!("Compilation failed");
            //         None
            //     }
            // }
            // match pico_8_cart_builder::get_lua_files(&src_dir.clone())
            //     .map(|entries| {
            //         entries.inspect(|entry| {
            //             let path = entry.path();
            //             let name = path
            //                 .file_name()
            //                 .map(|file_name| file_name.to_string_lossy().into_owned())
            //                 .unwrap_or_default();
            //             file_loading_tracker
            //                 .paths
            //                 .insert(name, FileLoadingState::Opened(path));
            //         })
            //     })
            //     .map(pico_8_cart_builder::dir_entries_to_tabs)
            //     .and_then(|tabs| pico_8_cart_builder::merge_tabs_with_src(&cart_path, tabs))
            // {
            //     Ok(cart) => Some(Message::CompilationOutput {
            //         compiled_data: Box::new(cart),
            //         cart_path,
            //     }),
            //     Err(e) => {
            //         tracing::error!("{e}");
            //         tracing::error!("Compilation failed");
            //         None
            //     }
            // }
            // match pico_8_cart_builder::get_lua_files(src_dir.as_path())
            //     .map(|entries| {
            //         entries.inspect(|entry| {
            //             let path = entry.path();
            //             let name = path.to_string_lossy().into_owned();
            //             file_loading_tracker
            //                 .paths
            //                 .insert(name, FileLoadingState::Opened(path));
            //         })
            //     })
            //     .map(pico_8_cart_builder::dir_entries_to_tabs)
            //     .map(pico_8_cart_builder::compile_tabs_to_cart_data)
            // {
            // }
            // match pico_8_cart_builder::CartBuilder::new(&src_dir).build(&cart_path) {
            //     Ok(cart) => Some(Message::CompilationOutput {
            //         compiled_data: Box::new(cart),
            //         cart_path,
            //     }),
            //     Err(e) => {
            //     }
            // }
        }
        Message::CompilationOutput {
            compiled_data: cart,
            cart_path,
        } => {
            // TODO: Add something here
            // Some(Message::IncomingLogLine("Compilation successful!".into()))
            tracing::info!("Compilation successful");
            let Ok(mut file) = fs::OpenOptions::new()
                .truncate(true)
                .write(true)
                .open(cart_path)
            else {
                tracing::error!("Failed to open output cart");
                return None;
            };
            let buf: Vec<u8> = cart.into_cart_source();
            tracing::debug!("Writing {} bytes to cart", buf.len());
            match io::Write::write_all(&mut file, buf.as_slice()) {
                Ok(_) => {
                    tracing::info!("Successfully wrote to cart");
                    None
                }
                Err(e) => {
                    tracing::error!("Failed to write to cart: {e}");
                    None
                }
            }
        }
        Message::Quit => {
            *running_state = RunningState::Done;
            None
        }
        Message::IncomingLogLine(log_message) => {
            log_messages.overwrite(log_message);
            None
            // log_messages.push_front(log_message);
            // log_messages
            //     .len()
            //     .ge(log_message_capacity)
            //     .then_some(Message::Reset)
        }
    }
}
fn view(
    Model {
        log_messages,
        file_loading_tracker,
        ..
    }: &Model,
    frame: &mut Frame,
) {
    use ratatui::widgets::{Block, Borders, List};

    let chunks = get_rect(frame, log_messages.len());

    frame.render_widget(Block::new().title("main").borders(Borders::ALL), chunks[0]);

    let file_loading_list = List::from_iter(file_loading_tracker.paths.iter().map(
        |(cartridge_name, state)| {
            Text::styled(
                format!("{cartridge_name}: {state:?}\n"),
                Style::new().italic(),
            )
            .centered()
        },
    ));

    frame.render_widget(file_loading_list, chunks[0]);

    let log_panel_chunk = chunks[1];
    frame.render_widget(ratatui::widgets::Clear, log_panel_chunk);
    // use crossterm::terminal::{Clear, ClearType};

    // use crossterm::terminal::{SetSize, size};
    // let cmd = size().map(|(x, y)| SetSize(x, y)).expect("size");
    // crossterm::execute!(io::stdout(), cmd).expect("failed cmd");
    // // crossterm::execute!(io::stdout(), Clear(ClearType::Purge)).expect("failed purge");
    let widget = LogPanelWidget::from_iter(
        log_messages
            .iter()
            .take(log_panel_chunk.height as usize)
            .cloned(),
    );
    frame.render_widget(widget, log_panel_chunk);
}
