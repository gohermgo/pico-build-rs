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

use pico_build_rs::FileData;

/// The size of the log-panel in log-lines
const LOG_LINE_COUNT: usize = 20;

struct ModelV2 {
    project_file: FileData<Box<pico_8_cart_model::CartData<'static>>>,
    source_directory: path::PathBuf,
    source_files: Box<[FileData<Box<[u8]>>]>,
    running_state: RunningState,
}

impl ModelV2 {
    fn new(cfg: config::AppConfiguration) -> io::Result<ModelV2> {
        let project_file = FileData::new(&cfg.cart_path());

        let mut app = ModelV2 {
            project_file,
            source_directory: cfg.src_dir,
            source_files: Box::default(),
            running_state: RunningState::Running,
        };

        Ok(app)
    }

    /// Discovers all source files in the configured directory
    fn discover_source_files(&self) -> io::Result<impl Iterator<Item = FileData<Box<[u8]>>>> {
        pico_build_rs::get_lua_files(self.source_directory.as_path())
            .map(pico_build_rs::dir_entries_to_source_files)
    }

    /// Reads the stateful files into memory
    fn read_source_files(&mut self) -> Result<(), pico_build_rs::FileDataError<Box<[u8]>>> {
        for source_file in self.source_files.iter_mut() {
            source_file.load()?;
        }

        Ok(())
    }

    /// Loads all source files in the configured directory
    fn load_source_files(&mut self) -> Result<(), pico_build_rs::FileDataError<Box<[u8]>>> {
        let source_files = self.discover_source_files().map(Box::from_iter)?;

        self.source_files = source_files;

        self.read_source_files()
    }

    /// Creates or loads the project-file
    fn load_project_file(
        &mut self,
    ) -> Result<(), pico_build_rs::FileDataError<Box<pico_8_cart_model::CartData<'static>>>> {
        tracing::debug!("Loading project file");
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

        if !self.source_files.iter().all(FileData::is_loaded) {
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
    let log_messages = pico_build_rs::Fifo::from(Box::from(log_lines));
    let log_panel_chunk = get_rect(&mut terminal.get_frame(), log_messages.len())[0];
    let mut model = Model {
        src_dir: cfg.src_dir.clone(),
        cart_path,
        log_message_rx: message_rx,
        log_messages,
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
            if matches!(
                current_message.as_ref(),
                Some(&Message::Input(InputMessage::ClearLog))
            ) {
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
    log_message_rx: mpsc::Receiver<log_panel::VisitPayload>,

    /// An owned log-message
    log_messages: pico_build_rs::Fifo<Line<'static>>,

    running_state: RunningState,
    file_loading_tracker: FileLoadingTracker,
}
#[derive(Debug)]
enum RunningState {
    Done,
    Running,
}
/// A user-command
#[derive(Debug)]
enum InputMessage {
    /// A request to analyze the cartridge was made
    Analyze {
        cart_path: path::PathBuf,
    },
    /// A request to compile the cartridge was made
    Compile {
        src_dir: path::PathBuf,
        cart_path: path::PathBuf,
    },
    /// A request to quit was made
    Quit,

    ClearLog,
}
/// A statement about how the model should change
#[derive(Debug)]
enum Message {
    Input(InputMessage),
    // /// A request to open
    // OpenCartridge {
    //     cartridge_directory_path: path::PathBuf,
    // },
    // /// A request to compile the cartridge was made
    // CompileCartridge {
    //     src_dir: path::PathBuf,
    //     cart_path: path::PathBuf,
    // },
    /// The compilation-request finished successfully
    CompilationOutput {
        compiled_data: Box<pico_8_cart_model::CartData<'static>>,
        cart_path: path::PathBuf,
    },

    /// A log-line has arrived
    IncomingLogLine(LogLine),
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
        return Some(Message::IncomingLogLine(Line::from(&log_message)));
    };

    match event::poll(Duration::from_millis(10)) {
        Err(_) | Ok(false) => None,
        Ok(true) => {
            if let Ok(Event::Key(key)) = event::read() {
                handle_key(model, key).map(Message::Input)
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
) -> Option<InputMessage> {
    match key.code {
        KeyCode::Enter if key.is_press() => Some(InputMessage::Compile {
            src_dir: src_dir.clone(),
            cart_path: cart_path.clone(),
        }),
        KeyCode::Char('q' | 'Q') => Some(InputMessage::Quit),
        KeyCode::Char('c' | 'C') => Some(InputMessage::ClearLog),
        _ => None,
    }
}
#[tracing::instrument(level = "trace", skip(log_messages, file_loading_tracker))]
fn update(
    Model {
        log_messages,
        running_state,
        file_loading_tracker,
        ..
    }: &mut Model,
    message: Message,
) -> Option<Message> {
    match message {
        Message::Input(input_message) => match input_message {
            InputMessage::ClearLog => {
                for message in log_messages.iter_mut() {
                    *message = Line::default();
                }
                log_messages.reset_cursor();
                None
            }

            InputMessage::Compile { src_dir, cart_path } => {
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
                    FileData::try_from(source_entry)
                        .map_err(pico_build_rs::FileDataError::Io)
                        .inspect_err(|e| tracing::error!("Failed to convert source-entry: {e:?}"))
                        .and_then(FileData::into_loaded_or_default)
                        .ok()
                });
                match FileData::new(cart_path.as_path())
                    .into_loaded_or_default()
                    .and_then(|cart_file| {
                        pico_build_rs::compile_cartridge(cart_file, source_files)
                            .map_err(Into::into)
                    }) {
                    Ok(cart) => {
                        tracing::info!("Got cart-data");
                        Some(Message::CompilationOutput {
                            compiled_data: Box::new(cart),
                            cart_path,
                        })
                    }
                    Err(e) => {
                        tracing::error!("Failed to compile {e:?}");
                        None
                    }
                }
            }
            InputMessage::Analyze { cart_path } => todo!("implement analyze"),
            InputMessage::Quit => {
                *running_state = RunningState::Done;
                None
            }
        },
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
