use core::ops::Deref;
use core::time::Duration;

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path;
use std::sync::mpsc;

use anyhow::anyhow;
use clap::Parser;
use pico_8_cart_model::CartData;
use pico_build_rs::Fifo;
use ratatui::prelude::*;

mod args;
mod config;
mod log_panel;

use log_panel::{LogPanelAction, LogPanelStore, LogPanelWidget};

pub trait StoreUpdate {
    type Action;

    fn update(&mut self, action: Self::Action) -> Option<Message>;
}

use pico_build_rs::FileData;

/// The size of the log-panel in log-lines
const LOG_LINE_COUNT: usize = 20;

#[derive(Debug)]
pub enum Action {
    UpdateLogPanel(LogEvent),
    ClearLogPanel,
    CompileCartridge,
    SaveCompiledCartridge {
        cartridge_data: Box<CartData<'static>>,
    },
    AnalyzeCartridge,
    DisplayAnalyzedCartridge {
        cartridge_data: Box<CartData<'static>>,
    },
    Quit,
}

pub struct ActionContext<'a> {
    log_panel_store: &'a mut LogPanelStore,
    running_state: &'a mut RunningState,
    project_source_file_path: &'a path::Path,
    project_source_directory_path: &'a path::Path,
}

impl Action {
    /// Invokes the action, and if a follow-up is needed,
    /// returns the next step
    #[tracing::instrument(level = "trace", ret)]
    pub fn invoke(
        self,
        ActionContext {
            log_panel_store,
            running_state,
            project_source_file_path,
            project_source_directory_path,
        }: ActionContext<'_>,
    ) -> Option<Action> {
        match self {
            Action::ClearLogPanel => {
                log_panel_store.clear();
                // TODO: Here we maybe wanna return a clear or redraw terminal action?
                None
            }
            Action::UpdateLogPanel(log_event) => {
                log_panel_store.update(log_event);
                // TODO: Here we maybe wanna return a clear or redraw terminal action?
                None
            }
            Action::CompileCartridge => {
                tracing::info!("Writing to cart-path {project_source_file_path:?}");
                let source_files =
                    match pico_build_rs::get_lua_files(project_source_directory_path) {
                        Ok(files) => files,
                        // files.inspect(|entry| {
                        //     let path = entry.path();
                        //     let name = path
                        //         .file_name()
                        //         .map(|file_name| file_name.to_string_lossy().into_owned())
                        //         .unwrap_or_default();
                        //     file_loading_tracker
                        //         .paths
                        //         .insert(name, FileLoadingState::Opened(path));
                        // }),
                        Err(e) => {
                            tracing::error!("Failed to get lua files {e}");
                            return None;
                        }
                    }
                    .filter_map(|source_entry| {
                        FileData::try_from(source_entry)
                            .map_err(pico_build_rs::FileDataError::Io)
                            .inspect_err(|e| {
                                tracing::error!("Failed to convert source-entry: {e:?}")
                            })
                            .and_then(FileData::into_loaded_or_default)
                            .ok()
                    });
                match FileData::new(project_source_file_path)
                    .into_loaded_or_default()
                    .and_then(|cart_file| {
                        pico_build_rs::compile_cartridge(cart_file, source_files)
                            .map_err(Into::into)
                    }) {
                    Ok(cart) => {
                        tracing::info!("Got cart-data");
                        Some(Action::SaveCompiledCartridge {
                            cartridge_data: Box::new(cart),
                        })
                    }
                    Err(e) => {
                        tracing::error!("Failed to compile {e:?}");
                        None
                    }
                }
            }
            Action::SaveCompiledCartridge { cartridge_data } => {
                let buf: Box<[u8]> = cartridge_data.into_cart_source();
                tracing::info!("Saving compiled cartridge (size: {})", buf.len());
                let Ok(mut file) = fs::OpenOptions::new()
                    .truncate(true)
                    .write(true)
                    .open(project_source_file_path)
                else {
                    tracing::info!("Failed to open output cart");
                    return None;
                };
                // tracing::info!("Writing {} bytes to cart", buf.len());
                match io::Write::write_all(&mut file, buf.as_ref()) {
                    Ok(_) => {
                        tracing::info!("Successfully wrote to cart");
                        // panic!();
                        None
                    }
                    Err(e) => {
                        tracing::error!("Failed to write to cart: {e}");
                        None
                    }
                }
            }
            Action::AnalyzeCartridge => {
                tracing::debug!("Pretend im analyzing a cartridge");
                None
            }
            Action::DisplayAnalyzedCartridge { cartridge_data } => {
                todo!("implement displaying analyzed cartridge")
            }
            Action::Quit => {
                *running_state = RunningState::Done;
                None
            }
        }
    }
}

#[derive(Debug)]
pub struct Store {
    log_panel_store: log_panel::LogPanelStore,
    workspace_store: WorkspaceStore,
    running_state: RunningState,
}

pub enum WorkspaceStoreAction {
    Compile,
    Analyze,
}

#[derive(Debug)]
struct WorkspaceStore {
    project_file: FileData<Box<pico_8_cart_model::CartData<'static>>>,
    source_directory: path::PathBuf,
    source_files: Box<[FileData<Box<[u8]>>]>,
    // running_state: RunningState,
}

impl WorkspaceStore {
    fn new(cfg: config::AppConfiguration) -> io::Result<WorkspaceStore> {
        let project_file = FileData::new(&cfg.cart_path());

        let mut app = WorkspaceStore {
            project_file,
            source_directory: cfg.src_dir,
            source_files: Box::default(),
            // running_state: RunningState::Running,
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

    /// Rediscovers, but does not load source files in the configured directory
    fn reset_source_files(&mut self) -> Result<(), pico_build_rs::FileDataError<Box<[u8]>>> {
        let source_files = self.discover_source_files().map(Box::from_iter)?;
        self.source_files = source_files;
        Ok(())
    }

    /// Loads all source files in the configured directory
    fn load_source_files(&mut self) -> Result<(), pico_build_rs::FileDataError<Box<[u8]>>> {
        self.reset_source_files()?;
        self.read_source_files()
    }

    fn reset_project_file(&mut self) {
        tracing::debug!("Resetting project file");
        self.project_file.unload();
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

impl StoreUpdate for WorkspaceStore {
    type Action = WorkspaceStoreAction;
    fn update(&mut self, action: Self::Action) -> Option<Message> {
        match action {
            WorkspaceStoreAction::Compile => todo!(),
            WorkspaceStoreAction::Analyze => todo!(),
        }
    }
}

impl Store {
    pub fn update(&mut self, action: Action) {
        todo!()
    }
}
// impl StoreUpdate for Store {
//     type Action = Action;
//     fn update(&mut self, action: Self::Action) -> Option<Message> {
//         match action {
//             Action::LogPanel(log_panel_action) => self.log_panel_store.update(log_panel_action),
//             Action::Quit => ,
//         }
//     }
// }

#[derive(Debug)]
struct ModelV2 {
    workspace: WorkspaceStore,
    running_state: RunningState,
    log_messages: Fifo<Line<'static>>,

    action_rx: mpsc::Receiver<Action>,
}

impl ModelV2 {
    // /// Checks for incoming actions and handles them accordingly
    // pub fn update(&self) -> Result<(), mpsc::TryRecvError> {
    //     'drain: loop {
    //         let Ok(action) = self.action_rx.try_recv() {
    //             match action {
    //                 Action::LogPanel(LogPanelAction::HandleIncoming(log_event)) =>
    //             }
    //         }
    //     }
    //     todo!()
    // }
}

#[tracing::instrument(level = "info", ret)]
fn main() -> anyhow::Result<()> {
    use crate::args::AppArgs;
    use crate::config::AppConfiguration;

    let log_event_rx = log_panel::setup_tracing_subscriber();

    let args = AppArgs::parse();

    let cfg = AppConfiguration::new(&args)?;
    tracing::info!("parsed app configuration");
    tracing::trace!("{cfg:#?}");
    tracing::info!("source directory is {:?}", cfg.src_dir);
    let cart_path = cfg.cart_path();
    tracing::info!("cart path is {:?}", cart_path);
    let mut terminal = ratatui::init();
    let log_panel_store = LogPanelStore::default();
    tracing::info!("log-messages length: {}", log_panel_store.len());
    // let log_panel_chunk = get_ui_rects(&mut terminal.get_frame(), log_messages.len())[1];
    // tracing::info!("log-panel height: {}", log_panel_chunk.height);
    // let (tx, rx) = mpsc::channel();
    let (action_tx, action_rx) = mpsc::channel();
    let mut event_bus = EventBus::new(action_tx);
    event_bus.register_listener(KeyboardEventListener::default());
    event_bus.register_listener(LogEventListener::new(log_event_rx));
    let _input_thread = std::thread::spawn(move || {
        // let event_bus = EventBus::new(action_tx);
        while let Ok(()) = event_bus.update() {
            // tracing::debug!("looping");
            // empty loop; keeps the thread alive and breaks on disconnected channel
        }
    });
    let mut model = Model {
        src_dir: cfg.src_dir.clone(),
        cart_path,
        log_panel_store,
        running_state: RunningState::Running,
        file_loading_tracker: FileLoadingTracker {
            paths: Default::default(),
        },
    };
    while !matches!(model.running_state, RunningState::Done) {
        terminal.draw(|frame| view(&model, frame))?;

        let mut current_action = match action_rx.try_recv() {
            Ok(val) => Some(val),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                break;
            }
        };

        while current_action.is_some() {
            let ctx = ActionContext {
                log_panel_store: &mut model.log_panel_store,
                running_state: &mut model.running_state,
                project_source_file_path: model.cart_path.as_path(),
                project_source_directory_path: model.src_dir.as_path(),
            };

            current_action = current_action.unwrap().invoke(ctx);
        }

        // let mut current_message = handle_event(&model)
        //     .or_else(|| next_message(&log_event_rx).map(Message::IncomingLogLine));

        // while current_message.is_some() {
        //     current_message = update(&mut model, current_message.unwrap())
        //         .or_else(|| next_message(&log_event_rx).map(Message::IncomingLogLine));
        //     if matches!(
        //         current_message.as_ref(),
        //         Some(&Message::Input(InputMessage::ClearLog))
        //     ) {
        //         // terminal.swap_buffers();
        //         // let rect = get_rect(&mut terminal.get_frame())[1];
        //         // let mut frame = terminal.get_frame();
        //         // let buffer = frame.buffer_mut();
        //         // for position in rect.positions() {
        //         //     buffer[position].set_symbol("JUNK");
        //         // }
        //         // terminal.swap_buffers();
        //         terminal.clear()?;
        //     }
        // }
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

use crossterm::event::{self, KeyEventKind};
use crossterm::event::{Event, KeyCode, KeyEvent};

use crate::log_panel::LogEvent;

fn layout(log_panel_lines: usize) -> Layout {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),                         // big main-box
            Constraint::Length(log_panel_lines as u16), // log-box
        ])
}
fn get_ui_rects(frame: &mut Frame, log_panel_lines: usize) -> std::rc::Rc<[Rect]> {
    layout(log_panel_lines).split(frame.area())
}

const LOG_PANEL_RECT_INDEX: usize = log_panel::RECT_INDEX;

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
    // /// Checked during the [`handle_event`] call
    // log_message_rx: mpsc::Receiver<log_panel::VisitPayload>,
    /// An owned log-message
    log_panel_store: LogPanelStore,

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
    /// The analysis-request finished successfully
    AnalysisOutput {
        analyzed_data: Box<pico_8_cart_model::CartData<'static>>,
    },
    /// The compilation-request finished successfully
    CompilationOutput {
        compiled_data: Box<pico_8_cart_model::CartData<'static>>,
        cart_path: path::PathBuf,
    },

    LogCleared,
    LogUpdated,
}

/// Make a decision regarding how the model should change
fn handle_event(model: &Model) -> Option<Message> {
    // if let Ok(log_message) = log_message_rx.try_recv() {
    //     return Some(Message::IncomingLogLine(Line::from(&log_message)));
    // };

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
        KeyCode::Char('a' | 'A') => Some(InputMessage::Analyze {
            cart_path: cart_path.clone(),
        }),
        KeyCode::Char('q' | 'Q') => Some(InputMessage::Quit),
        KeyCode::Char('c' | 'C') => Some(InputMessage::ClearLog),
        _ => None,
    }
}

fn read_event_polled(timeout: Duration) -> io::Result<Option<Event>> {
    let event_available =
        event::poll(timeout).inspect_err(|e| tracing::error!("Failed to poll for events: {e}"))?;
    if event_available {
        event::read().map(Some)
    } else {
        Ok(None)
    }
}

impl KeyboardEventListener {
    fn lookup_key_code(&self, key_code: &KeyCode) -> Option<&UserCommand> {
        self.key_map.get(key_code)
    }
    fn translate(&self, key_event: KeyEvent) -> Option<InputEvent> {
        self.lookup_key_code(&key_event.code)
            .copied()
            .map(|action_kind| InputEvent {
                user_command: action_kind,
                event_kind: key_event.kind,
            })
    }
    fn poll_next(&self) -> io::Result<Option<KeyEvent>> {
        match read_event_polled(Duration::from_millis(10)) {
            Ok(Some(Event::Key(key_event))) => Ok(Some(key_event)),
            Ok(_) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[derive(Default)]
pub enum EventListenerError<T> {
    #[default]
    NoneAvailable,
    Send(mpsc::SendError<T>),
    TrySend(mpsc::TrySendError<T>),
}

impl<T> From<mpsc::SendError<T>> for EventListenerError<T> {
    fn from(v: mpsc::SendError<T>) -> Self {
        Self::Send(v)
    }
}

impl<T> From<mpsc::TrySendError<T>> for EventListenerError<T> {
    fn from(v: mpsc::TrySendError<T>) -> Self {
        Self::TrySend(v)
    }
}

/// Listens for some type of event and maps into an [`Action`]
pub trait EventListener {
    /// Checks for the arrival of an event and tries to turn it
    /// into an action
    fn next_action(&self) -> Option<Action>;
    // /// Tries to get the next event
    // fn forward_incoming(
    //     &self,
    //     tx: &mpsc::Sender<Self::Listened>,
    // ) -> Result<bool, mpsc::SendError<Self::Listened>> {
    //     match self.get_next() {
    //         Some(event) => tx.send(event).map(|_| true),
    //         None => Ok(false),
    //     }
    // }
    // fn try_handle_incoming(
    //     &self,
    //     tx: &mpsc::SyncSender<Self::Listened>,
    // ) -> Result<bool, mpsc::TrySendError<Self::Listened>> {
    //     match self.get_next() {
    //         Some(event) => tx.try_send(event).map(|_| true),
    //         None => Ok(false),
    //     }
    // }
}

pub struct EventListenerBus(Vec<Box<dyn EventListener>>);
// impl EventListenerBus {
// }

pub struct EventBus {
    listeners: Vec<Box<dyn EventListener + Send>>,
    action_tx: mpsc::Sender<Action>,
}

impl EventBus {
    pub fn new(action_tx: mpsc::Sender<Action>) -> EventBus {
        EventBus {
            action_tx,
            listeners: vec![],
        }
    }
    pub fn register_listener<T>(&mut self, listener: T)
    where
        T: EventListener + Send + 'static,
    {
        self.listeners.push(Box::new(listener));
    }
    /// Checks all listeners for new actions and sends them
    pub fn update(&self) -> Result<(), mpsc::SendError<Action>> {
        for action in self
            .listeners
            .iter()
            .map(Box::as_ref)
            .filter_map(EventListener::next_action)
        {
            self.action_tx.send(action)?;
        }
        Ok(())
    }
    pub fn next_actions(&self) -> impl Iterator<Item = Action> {
        self.listeners
            .iter()
            .map(Box::as_ref)
            .filter_map(EventListener::next_action)
    }
}

#[derive(Copy, Clone, Debug)]
pub enum UserCommand {
    Compile,
    Analyze,
    Quit,
    ClearLog,
}

pub enum InputActionState {
    Press,
    Release,
    Repeat,
}

#[derive(Copy, Clone, Debug)]
pub struct InputEvent {
    user_command: UserCommand,
    event_kind: KeyEventKind,
}

#[derive(Debug)]
pub struct KeyboardEventListener {
    key_map: HashMap<KeyCode, UserCommand>,
}

impl Default for KeyboardEventListener {
    fn default() -> Self {
        KeyboardEventListener {
            key_map: HashMap::from([
                (KeyCode::Enter, UserCommand::Compile),
                (KeyCode::Char('a'), UserCommand::Analyze),
                (KeyCode::Char('A'), UserCommand::Analyze),
                (KeyCode::Char('q'), UserCommand::Quit),
                (KeyCode::Char('Q'), UserCommand::Quit),
                (KeyCode::Char('c'), UserCommand::ClearLog),
                (KeyCode::Char('C'), UserCommand::ClearLog),
            ]),
        }
    }
}

impl EventListener for KeyboardEventListener {
    fn next_action(&self) -> Option<Action> {
        let Ok(Some(next_key_event)) = self.poll_next() else {
            return None;
        };

        let InputEvent {
            user_command,
            event_kind,
        } = self.translate(next_key_event)?;

        event_kind.is_press().then_some(match user_command {
            UserCommand::ClearLog => Action::ClearLogPanel,
            UserCommand::Compile => Action::CompileCartridge,
            UserCommand::Analyze => Action::AnalyzeCartridge,
            UserCommand::Quit => Action::Quit,
        })
    }
}

pub struct LogEventListener {
    log_event_rx: mpsc::Receiver<LogEvent>,
}

impl LogEventListener {
    pub fn new(log_event_rx: mpsc::Receiver<LogEvent>) -> LogEventListener {
        LogEventListener { log_event_rx }
    }
}

impl EventListener for LogEventListener {
    fn next_action(&self) -> Option<Action> {
        self.log_event_rx
            .try_recv()
            .map(Action::UpdateLogPanel)
            .ok()
    }
}

trait CrosstermEventHandler {
    fn event_filter(&self) -> Box<dyn Fn(&Event) -> bool>;
    fn handle_event(&self, event: Event);

    // type MappedEvent;
    // fn event_filter_map(&self) -> Box<dyn Fn(&Event) -> Option<Self::MappedEvent>>;
}
pub struct DispatchMap(HashMap<fn(&Event) -> bool, Box<dyn CrosstermEventHandler>>);
// impl Cr
// impl<H> CrosstermEventHandler for H where H: EventListener, <H as EventListener>::Listened: TryFrom<Event> {
//     fn event_filter(&self) -> Box<dyn Fn(&Event) -> bool> {
//         Box::new(|event| {
//             <Self as EventListener>::Listened::try_from(event.clone()).is_ok()
//         })
//     }
//     fn consume_event(&self, event: Event) {
//         if let Ok(specific_event) = <Self as EventListener>::Listened::try_from(event) {
//             self.forward_incoming(tx)
//         }
//     }
// }

pub struct InputEventHandler {
    listener: KeyboardEventListener,
    action_tx: mpsc::Sender<Action>,
}

// impl InputEventHandler {

// }

// impl CrosstermEventHandler for InputEventHandler {
//     fn event_filter(&self) -> Box<dyn Fn(&Event) -> bool> {
//         Box::new(|event| matches!(event, &Event::Key(_)))
//     }
//     fn handle_event(&self, event: Event) {
//         let Event::Key(key_event) =
//     }
// }

#[derive(Debug)]
pub struct CrosstermEventDispatcher {
    input_event_listener: KeyboardEventListener,
    input_event_tx: mpsc::Sender<InputEvent>,
}

// trait EventDispatcher {}

#[derive(Debug)]
pub struct Dispatcher {
    action_tx: mpsc::Sender<Action>,

    input_event_rx: mpsc::Receiver<InputEvent>,
}

impl Dispatcher {
    pub fn update(&self) -> Result<(), mpsc::SendError<Action>> {
        if let Ok(InputEvent {
            user_command: action,
            event_kind,
        }) = self.input_event_rx.try_recv()
        {
            let action = match action {
                UserCommand::Analyze => todo!("analyze action"),
                UserCommand::ClearLog => todo!("clear log action"),
                UserCommand::Quit => todo!("quit action"),
                UserCommand::Compile => todo!("compile action"),
            };
            todo!()
        }
        todo!()
    }
}

// impl CrosstermEventDispatcher {
//     pub fn new(input_event_tx: mpsc::Sender<InputEvent>) -> CrosstermEventDispatcher {

//     }
//     fn dispatch_event(&self, event: Event) {
//         match event {
//             Event::Key(key_event) => {
//                 self.input_event_listener
//                     .forward_incoming(&self.input_event_tx);
//             }
//             _ => {}
//         }
//     }
// }

#[tracing::instrument(level = "trace", skip(log_panel_store, file_loading_tracker))]
fn update(
    Model {
        log_panel_store,
        running_state,
        file_loading_tracker,
        ..
    }: &mut Model,
    message: Message,
) -> Option<Message> {
    match message {
        Message::Input(input_message) => match input_message {
            InputMessage::ClearLog => {
                log_panel_store.clear();
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
            InputMessage::Analyze { cart_path } => {
                tracing::info!("TODO: implement analyze ({cart_path:?})");
                None
            }
            InputMessage::Quit => {
                *running_state = RunningState::Done;
                None
            }
        },
        Message::AnalysisOutput { analyzed_data } => todo!("implement analysis-output widget"),
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
                tracing::info!("Failed to open output cart");
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

        Message::LogCleared => todo!("log cleared"),
        Message::LogUpdated => todo!("log updated"),
        // Message::IncomingLogLine(log_message) => {
        //     log_messages.overwrite(log_message);
        //     None
        //     // log_messages.push_front(log_message);
        //     // log_messages
        //     //     .len()
        //     //     .ge(log_message_capacity)
        //     //     .then_some(Message::Reset)
        // }
    }
}
fn view(
    Model {
        log_panel_store: log_messages,
        file_loading_tracker,
        ..
    }: &Model,
    frame: &mut Frame,
) {
    use ratatui::widgets::{Block, Borders, List};

    let chunks = get_ui_rects(frame, log_messages.len());

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
