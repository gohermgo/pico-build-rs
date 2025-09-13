use core::fmt;

use std::io;
use std::sync::mpsc;

use ratatui::prelude::*;
use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

pub struct LogPanelWidget<'a> {
    log_lines: Vec<Line<'a>>,
}

impl<'a> From<Vec<Line<'a>>> for LogPanelWidget<'a> {
    fn from(value: Vec<Line<'a>>) -> Self {
        LogPanelWidget { log_lines: value }
    }
}

impl<'a> FromIterator<Line<'a>> for LogPanelWidget<'a> {
    fn from_iter<T: IntoIterator<Item = Line<'a>>>(iter: T) -> Self {
        let log_lines: Vec<Line<'a>> = iter.into_iter().collect();
        LogPanelWidget::from(log_lines)
    }
}

impl Widget for LogPanelWidget<'_> {
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
struct SenderLayer {
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
        event.record(&mut SendingVisitor {
            message_tx: self.message_tx.clone(),
            metadata: _ctx.current_span().metadata(),
        });
    }
}
struct WriteIntoSender<'a> {
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
struct SendingVisitor<'ctx> {
    message_tx: mpsc::Sender<String>,
    metadata: Option<&'ctx tracing::Metadata<'static>>,
}
impl Visit for SendingVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.record_str(field, format!("{value:?}").as_str())
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        let metadata = self
            .metadata
            .map(|md| md.target().to_string())
            .unwrap_or_else(|| "main".into());
        let format = match field.name() {
            "return" => format!("returned {value}"),
            "message" => value.to_string(),
            _ => format!("{field}={value}"),
        };
        match self
            .message_tx
            .send(format!("[{metadata}:{}] {format}", field.index()))
        {
            Ok(_) => {}
            Err(e) => eprintln!("Failed to send event from sending-visitor: {e}"),
        }
    }
}

/// Returns a channel for the messages (u probably want em)
pub fn setup_tracing_subscriber() -> mpsc::Receiver<String> {
    let (message_tx, message_rx) = mpsc::channel();

    tracing_subscriber::registry()
        .with(SenderLayer { message_tx })
        .init();

    message_rx
}
