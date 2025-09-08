use core::fmt;
use core::time::Duration;

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::sync::mpsc;

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
    let mut log_state = LogState {
        buf: VecDeque::with_capacity(50),
        cap: 50,
    };

    let args = AppArgs::parse();

    let cfg = AppConfiguration::new(&args)?;
    tracing::info!("parsed app configuration");
    tracing::trace!("{cfg:#?}");

    let cart_path = cfg
        .cart_path()
        .ok_or_else(|| anyhow!("Configuration invalid, cart path does not point to a file"))?;
    let mut terminal = ratatui::init();
    loop {
        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1),         // big main-box
                    Constraint::Percentage(50), // log-box
                ])
                .split(frame.area());
            frame.render_widget(
                widgets::Block::new()
                    .title("main")
                    .borders(widgets::Borders::ALL),
                chunks[0],
            );

            if let Ok(msg) = message_rx.try_recv() {
                log_state.push(msg);
            }
            frame.render_stateful_widget(LogWidget {}, chunks[1], &mut log_state);
            // frame.render_widget(
            //     widgets::Paragraph::new(logs.iter().enumerate().fold(
            //         Default::default(),
            //         |acc, (index, elt)| {
            //             if index == 0 {
            //                 elt.to_string()
            //             } else {
            //                 format!("{acc}\n{elt}")
            //             }
            //         },
            //     ))
            //     .block(
            //         widgets::Block::new()
            //             .title("logs")
            //             .borders(widgets::Borders::ALL),
            //     ),
            //     chunks[1],
            // );
            // frame.render_widget(widget, area);
        })?;
        if event::poll(Duration::from_millis(10))? {
            match event::read()? {
                event::Event::Key(key) if key.kind == event::KeyEventKind::Press => {
                    match key.code {
                        event::KeyCode::Char('q') => break,
                        event::KeyCode::Enter => {
                            let cart = pico_8_cart_builder::CartBuilder::new(&cfg.src_dir)
                                .build(&cart_path)?;
                            tracing::info!("got cart");
                            tracing::trace!("{cart:#?}")
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
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

    ratatui::restore();
    Ok(())
}

use ratatui::widgets::StatefulWidget;
use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{Layer, fmt::FormatEvent};
use tracing_subscriber::{fmt::MakeWriter, layer::SubscriberExt};

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
pub struct SendingVisitor {
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

    tracing_subscriber::fmt()
        .with_writer(SenderLayer { message_tx })
        .with_max_level(tracing::level_filters::STATIC_MAX_LEVEL)
        .with_ansi(true)
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

        let amount_off_screen = state.buf.len().checked_sub(height).unwrap_or_default();

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
