use core::fmt;
use core::time::Duration;

use std::fs;
use std::io;
use std::sync::mpsc;
use std::{collections::VecDeque, path};

use anyhow::anyhow;
use clap::Parser;
use ratatui::prelude::*;

mod args;
mod config;

#[tracing::instrument(level = "info", ret)]
fn main() -> anyhow::Result<()> {
    use crate::args::AppArgs;
    use crate::config::AppConfiguration;
    use crossterm::event;
    use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
    use ratatui::widgets;

    let message_rx = setup_subscriber();

    let args = AppArgs::parse();

    let cfg = AppConfiguration::new(&args)?;
    tracing::info!("parsed app configuration");
    tracing::trace!("{cfg:#?}");

    tracing::info!("source directory is {:?}", cfg.src_dir);
    let cart_path = cfg
        .cart_path()
        .ok_or_else(|| anyhow!("Configuration invalid, cart path does not point to a file"))?;
    tracing::info!("output path is {:?}", cart_path);
    let mut terminal = ratatui::init();
    let log_panel_chunk = get_rect(&mut terminal.get_frame())[0];
    let mut model = Model {
        src_dir: cfg.src_dir.clone(),
        cart_path,
        log_message_rx: message_rx,
        log_messages: VecDeque::new(),
        log_message_capacity: log_panel_chunk.height as usize,
        running_state: RunningState::Running,
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
use ratatui::widgets::StatefulWidget;
use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{Layer, fmt::FormatEvent};
use tracing_subscriber::{fmt::MakeWriter, layer::SubscriberExt};
fn layout() -> Layout {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),         // big main-box
            Constraint::Percentage(50), // log-box
        ])
}
fn get_rect(frame: &mut Frame) -> std::rc::Rc<[Rect]> {
    layout().split(frame.area())
}
/// Subject to change (might want to use [`Line`] instead)
type LogLine = Line<'static>;
struct Model {
    src_dir: path::PathBuf,
    cart_path: path::PathBuf,
    /// Checked during the [`handle_event`] call
    log_message_rx: mpsc::Receiver<String>,

    /// An owned log-message
    log_messages: VecDeque<LogLine>,
    /// The implied capacity for log-messages
    log_message_capacity: usize,

    running_state: RunningState,
}
enum RunningState {
    Done,
    Running,
}
/// A statement about how the model should change
enum Message {
    /// A request to compile the cartridge was made
    CompileCartridge,

    /// The compilation-request finished successfully
    CompilationOutput(Box<pico_8_cart_model::CartData<'static>>),

    /// A request to quit was made
    Quit,

    /// A log-line has arrived
    IncomingLogLine(LogLine),

    ClearLog,

    Reset,
}

// struct Message {
//     /// The log needs an update, and the terminal might need a forced redraw
//     incoming_log_message: Option<String>,
//     /// The user has made a request, some action needs to be taken
//     user_request: Option<UserRequest>,
// }

/// Make a decision regarding how the model should change
fn handle_event(Model { log_message_rx, .. }: &Model) -> Option<Message> {
    if let Ok(log_message) = log_message_rx.try_recv() {
        return Some(Message::IncomingLogLine(log_message.into()));
    };

    match event::poll(Duration::from_millis(10)) {
        Err(_) | Ok(false) => None,
        Ok(true) => {
            if let Ok(Event::Key(key)) = event::read() {
                handle_key(key)
            } else {
                None
            }
        }
    }
}
fn handle_key(key: KeyEvent) -> Option<Message> {
    match key.code {
        KeyCode::Enter if key.is_press() => Some(Message::CompileCartridge),
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
        cart_path,
        src_dir,
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
            log_messages.clear();
            None
        }
        Message::CompileCartridge => {
            match pico_8_cart_builder::CartBuilder::new(&src_dir).build(&cart_path) {
                Ok(cart) => Some(Message::CompilationOutput(Box::new(cart))),
                Err(e) => {
                    tracing::error!("{e}");
                    tracing::error!("Compilation failed");
                    None
                }
            }
        }
        Message::CompilationOutput(cart) => {
            // TODO: Add something here
            // Some(Message::IncomingLogLine("Compilation successful!".into()))
            tracing::info!("Compilation successful");
            None
        }
        Message::Quit => {
            *running_state = RunningState::Done;
            None
        }
        Message::IncomingLogLine(log_message) => {
            log_messages.push_front(log_message);
            log_messages
                .len()
                .ge(log_message_capacity)
                .then_some(Message::Reset)
        }
        Message::Reset => {
            while log_messages.len() >= *log_message_capacity {
                log_messages.pop_back();
            }
            None
        }
    }
}
fn view(Model { log_messages, .. }: &Model, frame: &mut Frame) {
    use ratatui::widgets::{Block, Borders};

    let chunks = get_rect(frame);

    frame.render_widget(Block::new().title("main").borders(Borders::ALL), chunks[0]);

    let log_panel_chunk = chunks[1];
    frame.render_widget(ratatui::widgets::Clear, log_panel_chunk);
    // use crossterm::terminal::{Clear, ClearType};

    // use crossterm::terminal::{SetSize, size};
    // let cmd = size().map(|(x, y)| SetSize(x, y)).expect("size");
    // crossterm::execute!(io::stdout(), cmd).expect("failed cmd");
    // // crossterm::execute!(io::stdout(), Clear(ClearType::Purge)).expect("failed purge");

    frame.render_widget(
        LogPanelWidget {
            log_lines: log_messages
                .iter()
                .take(log_panel_chunk.height as usize)
                .cloned()
                .collect(),
        },
        log_panel_chunk,
    );
}
struct LogPanelWidget {
    log_lines: Vec<LogLine>,
}
impl Widget for LogPanelWidget {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        use ratatui::widgets::{Block, Paragraph};
        Paragraph::new(self.log_lines)
            .block(Block::bordered().title("log-panel"))
            .render(area, buf)
    }
}
/// Just intercepts the messages and forwards them to the frontend bits
#[derive(Debug)]
pub struct SenderLayer {
    message_tx: mpsc::Sender<String>,
}
impl<S> Layer<S> for SenderLayer
where
    S: Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        // let fmt = tracing_subscriber::fmt::format::Format::default()
        //     .with_ansi(true)
        //     .format_event(ctx, writer, event);
        event.record(&mut SendingVisitor {
            message_tx: self.message_tx.clone(),
        });
    }
}
pub struct WriteIntoSender<'a> {
    tx: &'a mpsc::Sender<String>,
}
impl std::io::Write for WriteIntoSender<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let s = core::str::from_utf8(buf)
            .map_err(|e| std::io::Error::new(io::ErrorKind::InvalidData, e))?;
        let len = s.len();
        self.tx
            .send(s.to_string())
            .map_err(|e| std::io::Error::new(io::ErrorKind::HostUnreachable, e))?;
        Ok(len)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl<'a> MakeWriter<'a> for SenderLayer {
    type Writer = WriteIntoSender<'a>;
    fn make_writer(&'a self) -> Self::Writer {
        WriteIntoSender {
            tx: &self.message_tx,
        }
    }
}
struct SendingVisitor {
    message_tx: mpsc::Sender<String>,
}
impl Visit for SendingVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.record_str(field, format!("{value:?}").as_str())
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        match self.message_tx.send(format!("{field}={value}")) {
            Ok(_) => {}
            Err(e) => eprintln!("Failed to send event from sending-visitor: {e}"),
        }
    }
}

/// Returns a channel for the messages (u probably want em)
fn setup_subscriber() -> mpsc::Receiver<String> {
    let (message_tx, message_rx) = mpsc::channel();

    tracing_subscriber::registry()
        .with(SenderLayer { message_tx })
        // .with_max_level(tracing::level_filters::STATIC_MAX_LEVEL)
        // .compact()
        // .with_target(false)
        // .with_file(false)
        // .with_level(false)
        // .with_line_number(false)
        // .with_ansi(true)
        .init();

    // tracing_subscriber::registry()
    //     .with(SenderLayer { message_tx })
    //     .init();

    message_rx
}
struct LogWidget {}
struct LogState {
    buf: VecDeque<String>,
    cap: usize,
}
impl LogState {
    pub fn push(&mut self, elt: String) -> Option<String> {
        let ret = if self.cap == self.buf.len() {
            self.buf.pop_front()
        } else {
            None
        };
        self.buf.push_back(elt);
        ret
    }
}
impl StatefulWidget for LogWidget {
    type State = LogState;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        use ratatui::widgets;
        let height = area.height as usize;

        let messages_queued = state.buf.len();

        // let messages_to_take = match height.checked_sub(messages_queued) {
        //     // No need to redraw here
        //     Some(val) => val,
        //     /// This implies that
        //     None => usize::default(),
        // };

        let amount_off_screen = match state.buf.len().checked_sub(height) {
            // There are less messages in the queue than can be on screen
            // no need to redraw
            Some(val) => val,
            None => {
                // crossterm::execute!();

                usize::default()
            }
        };

        // let amount_off_screen = match state.buf.len().checked_sub(height) {
        //     // No need to redraw here
        //     Some(val) => val,
        //     /// This implies that
        //     None => {

        //         usize::default()
        //     }
        // };

        let message_iter = state.buf.iter().skip(amount_off_screen);

        let text = state
            .buf
            .iter()
            .skip(amount_off_screen)
            .fold(String::default(), |acc, elt| format!("{acc}{elt}"));

        widgets::Paragraph::new(text)
            .block(
                widgets::Block::new()
                    .title("logs")
                    .borders(widgets::Borders::ALL),
            )
            .render(area, buf);
    }
}
