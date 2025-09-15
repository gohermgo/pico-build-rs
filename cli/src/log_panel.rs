use core::fmt;

use std::sync::mpsc;

use ratatui::prelude::*;
use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

pub struct LogPanelWidget {
    log_lines: Vec<Line<'static>>,
}

impl From<Vec<Line<'static>>> for LogPanelWidget {
    fn from(value: Vec<Line<'static>>) -> Self {
        LogPanelWidget { log_lines: value }
    }
}

impl FromIterator<Line<'static>> for LogPanelWidget {
    fn from_iter<T: IntoIterator<Item = Line<'static>>>(iter: T) -> Self {
        let log_lines: Vec<Line<'static>> = iter.into_iter().collect();
        LogPanelWidget::from(log_lines)
    }
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

impl<'a> From<&'a VisitData> for std::borrow::Cow<'a, str> {
    fn from(value: &'a VisitData) -> Self {
        match value {
            VisitData::Debug(val) | VisitData::Str(val) => std::borrow::Cow::Borrowed(val),
        }
    }
}

#[derive(Debug)]
pub struct VisitMetadata {
    level: tracing::Level,
    target: &'static str,
    span_name: &'static str,
    line_number: Option<u32>,
}

/// Stylizes and constructs a [`Span`] with the [`tracing::Level`]
struct LevelSpan(tracing::Level);

impl From<LevelSpan> for Span<'static> {
    fn from(LevelSpan(level): LevelSpan) -> Self {
        Span::styled(
            level.to_string(),
            Style::new().fg(match level {
                tracing::Level::ERROR => Color::Red,
                tracing::Level::WARN => Color::Yellow,
                tracing::Level::INFO => Color::Green,
                tracing::Level::DEBUG => Color::Blue,
                tracing::Level::TRACE => Color::DarkGray,
            }),
        )
    }
}

struct SourceSpan<'a> {
    span_name: &'static str,
    line_number: Option<&'a u32>,
}

impl From<SourceSpan<'_>> for Span<'static> {
    fn from(
        SourceSpan {
            span_name,
            line_number,
        }: SourceSpan<'_>,
    ) -> Self {
        match line_number {
            Some(number) => Span::raw(format!("{span_name} @ {number}")),
            None => Span::raw(span_name),
        }
    }
}

impl core::fmt::Display for VisitMetadata {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("{}", Line::from(self)))
    }
}

impl From<&tracing::Metadata<'static>> for VisitMetadata {
    fn from(value: &tracing::Metadata<'static>) -> Self {
        VisitMetadata {
            target: value.target(),
            level: *value.level(),
            span_name: value.name(),
            line_number: value.line(),
        }
    }
}

impl VisitMetadata {
    pub fn spans(&self) -> impl Iterator<Item = Span<'static>> {
        [
            // Prints level and colorizes correctly
            LevelSpan(self.level).into(),
            // Add the target in
            Span::raw(format!(" | {} | ", self.target)),
            // Describes the message primarily
            SourceSpan {
                span_name: self.span_name,
                line_number: self.line_number.as_ref(),
            }
            .into(),
        ]
        .into_iter()
    }
}

impl From<&VisitMetadata> for Line<'static> {
    fn from(visit_metadata: &VisitMetadata) -> Self {
        Line::default().spans(visit_metadata.spans())
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

struct VisitDataSpan<'a> {
    field: &'a Field,
    data: &'a VisitData,
}

impl From<VisitDataSpan<'_>> for Span<'static> {
    fn from(VisitDataSpan { field, data }: VisitDataSpan<'_>) -> Self {
        let field_name = field.name();
        let data_str: std::borrow::Cow<'_, str> = data.into();
        let data_str = std::borrow::Cow::Owned(data_str.into_owned());
        match field_name {
            "return" => Span::styled(data_str, Style::new().fg(Color::DarkGray).italic()),
            "message" => Span::raw(data_str),
            _ => Span::raw(format!("{field_name}={data_str}")),
        }
    }
}

impl From<VisitPayload> for Line<'static> {
    fn from(ref value: VisitPayload) -> Self {
        value.into()
    }
}

impl From<&VisitPayload> for Line<'static> {
    fn from(
        VisitPayload {
            field,
            data,
            metadata,
        }: &VisitPayload,
    ) -> Self {
        let mut line_builder = if let Some(metadata) = metadata {
            Line::default().spans(
                core::iter::once(Span::from("[ "))
                    .chain(metadata.spans())
                    .chain(core::iter::once(Span::from(" ] "))),
            )
        } else {
            Line::default()
        };

        line_builder.push_span(VisitDataSpan { field, data });

        line_builder
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
    fn send_payload<'p, P: ?Sized>(&self, field: &Field, payload_data: &'p P)
    where
        VisitData: From<&'p P>,
    {
        match self
            .message_tx
            .send(VisitPayload::new(field, self.metadata, payload_data))
        {
            Ok(_) => {}
            Err(e) => eprintln!("Failed to send event from sending-visitor: {e}"),
        }
    }
}

impl Visit for SendingVisitor<'_, VisitPayload> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.send_payload(field, value);
        // match self.send_payload(field, value) {
        //     Ok(_) => {}
        //     Err(e) => eprintln!("Failed to send event from sending-visitor: {e}"),
        // }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.send_payload(field, value);
        // match self.send_payload(field, value) {
        //     Ok(_) => {}
        //     Err(e) => eprintln!("Failed to send event from sending-visitor: {e}"),
        // }
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
