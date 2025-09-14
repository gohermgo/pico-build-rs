use core::fmt;

use std::sync::mpsc;

use ratatui::prelude::*;
use tracing::Subscriber;
use tracing::field::{Field, Visit};
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
struct SenderLayer<T> {
    message_tx: mpsc::Sender<T>,
}

impl<S> Layer<S> for SenderLayer<VisitPayload>
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

#[derive(Debug)]
pub enum VisitData {
    /// Contains the debug string after formatting
    Debug(Box<str>),
    Str(Box<str>),
}

impl From<&dyn fmt::Debug> for VisitData {
    fn from(value: &dyn fmt::Debug) -> Self {
        VisitData::Debug(format!("{value:?}").into_boxed_str())
    }
}

impl From<Box<str>> for VisitData {
    fn from(v: Box<str>) -> Self {
        Self::Str(v)
    }
}

impl From<&str> for VisitData {
    fn from(value: &str) -> Self {
        let boxed: Box<str> = value.into();
        VisitData::from(boxed)
    }
}

#[derive(Debug)]
pub struct VisitMetadata {
    target: &'static str,
    span_name: &'static str,
    line_number: Option<u32>,
}

impl Default for VisitMetadata {
    fn default() -> Self {
        VisitMetadata {
            target: "main",
            span_name: Default::default(),
            line_number: Default::default(),
        }
    }
}

impl core::fmt::Display for VisitMetadata {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let VisitMetadata {
            target,
            span_name,
            line_number,
        } = self;

        let span_format = match line_number {
            Some(number) => format!("{span_name} @ {number}"),
            None => span_name.to_string(),
        };

        f.write_fmt(format_args!("{target} | {span_format}"))
    }
}

impl From<&tracing::Metadata<'static>> for VisitMetadata {
    fn from(value: &tracing::Metadata<'static>) -> Self {
        VisitMetadata {
            target: value.target(),
            span_name: value.name(),
            line_number: value.line(),
        }
    }
}

#[derive(Debug)]
pub struct VisitPayload {
    field: Field,
    data: VisitData,
    metadata: Option<VisitMetadata>,
}

impl core::fmt::Display for VisitPayload {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let VisitPayload {
            field,
            data,
            metadata,
        } = self;

        fn formatter<T: core::fmt::Display + ?Sized>(
            field_name: &str,
        ) -> impl FnOnce(&T) -> String {
            move |value| match field_name {
                "return" => format!("returned {value}"),
                "message" => value.to_string(),
                _ => format!("{field_name}={value}"),
            }
        }

        let value = match data {
            VisitData::Debug(d) => formatter(field.name())(d),
            VisitData::Str(s) => formatter(field.name())(s),
        };

        if let Some(metadata) = metadata {
            f.write_fmt(format_args!("[{metadata}] {value}"))
        } else {
            f.write_str(value.as_str())
        }
    }
}

impl VisitPayload {
    pub fn new<'p, P: ?Sized>(
        field: &Field,
        metadata: Option<&tracing::Metadata<'static>>,
        payload_data: &'p P,
    ) -> VisitPayload
    where
        VisitData: From<&'p P>,
    {
        VisitPayload {
            field: field.clone(),
            data: VisitData::from(payload_data),
            metadata: metadata.map(VisitMetadata::from),
        }
    }
}

#[derive(Debug)]
struct SendingVisitor<'ctx, T> {
    message_tx: mpsc::Sender<T>,
    metadata: Option<&'ctx tracing::Metadata<'static>>,
}

impl<'ctx> SendingVisitor<'ctx, VisitPayload> {
    fn send_payload<'p, P: ?Sized>(
        &self,
        field: &Field,
        payload_data: &'p P,
    ) -> Result<(), mpsc::SendError<VisitPayload>>
    where
        VisitData: From<&'p P>,
    {
        let payload = VisitPayload::new(field, self.metadata, payload_data);
        self.message_tx.send(payload)
    }
}

impl Visit for SendingVisitor<'_, VisitPayload> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        match self.send_payload(field, value) {
            Ok(_) => {}
            Err(e) => eprintln!("Failed to send event from sending-visitor: {e}"),
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        match self.send_payload(field, value) {
            Ok(_) => {}
            Err(e) => eprintln!("Failed to send event from sending-visitor: {e}"),
        }
        // let metadata = self
        //     .metadata
        //     .map(|md| md.target().to_string())
        //     .unwrap_or_else(|| "main".into());
        // let format = match field.name() {
        //     "return" => format!("returned {value}"),
        //     "message" => value.to_string(),
        //     _ => format!("{field}={value}"),
        // };
        // match self
        //     .message_tx
        //     .send(format!("[{metadata}:{}] {format}", field.index()))
        // {
        //     Ok(_) => {}
        //     Err(e) => eprintln!("Failed to send event from sending-visitor: {e}"),
        // }
    }
}

/// Returns a channel for the messages (u probably want em)
pub fn setup_tracing_subscriber() -> mpsc::Receiver<VisitPayload> {
    let (message_tx, message_rx) = mpsc::channel();

    tracing_subscriber::registry()
        .with(SenderLayer { message_tx })
        .init();

    message_rx
}
