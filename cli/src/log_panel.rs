use core::fmt;
use core::ops::{Deref, DerefMut};

use std::sync::mpsc;

use pico_build_rs::Fifo;
use ratatui::{
    prelude::*,
    widgets::{Block, Padding, Paragraph},
};
use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Debug)]
pub struct LogPanelStore {
    buf: Fifo<Line<'static>>,
}

impl Deref for LogPanelStore {
    type Target = Fifo<Line<'static>>;

    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

// impl DerefMut for LogPanelStore {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         &mut self.buf
//     }
// }

impl FromIterator<Line<'static>> for LogPanelStore {
    fn from_iter<T: IntoIterator<Item = Line<'static>>>(iter: T) -> Self {
        let buf = Box::from_iter(iter);
        let buf = Fifo::from(buf);
        LogPanelStore { buf }
    }
}

impl Default for LogPanelStore {
    fn default() -> Self {
        let arr: [Line<'static>; LINE_COUNT] = core::array::from_fn(|_| Line::default());
        LogPanelStore {
            buf: Fifo::from_iter(arr),
        }
    }
}

impl LogPanelStore {
    pub fn clear(&mut self) {
        for log_line in self.buf.iter_mut() {
            *log_line = Line::default();
        }
        self.buf.reset_cursor();
    }
    pub fn update(&mut self, log_event: LogEvent) {
        self.buf.overwrite(Line::from(log_event));
    }
}

#[derive(Debug)]
pub enum LogPanelAction {
    HandleIncoming(LogEvent),
    Clear,
}

// impl crate::StoreUpdate for LogPanelStore {
//     type Action = LogPanelAction;

//     #[tracing::instrument(level = "debug", skip(self))]
//     fn update(&mut self, action: Self::Action) {
//         match action {
//             LogPanelAction::HandleIncoming(incoming_payload) => {
//                 self.buf.overwrite(Line::from(incoming_payload));
//                 Some(crate::Message::LogUpdated)
//             }
//             LogPanelAction::Clear => {
//                 for log_line in self.buf.iter_mut() {
//                     *log_line = Line::default();
//                 }
//                 self.buf.reset_cursor();
//                 Some(crate::Message::LogCleared)
//             }
//         }
//     }
// }

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
pub fn get_block() -> Block<'static> {
    Block::bordered().title("log-panel")
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct LogPanelBlock<'a> {
    inner: Block<'a>,
    log_panel_lines: usize,
}

impl<'a> Deref for LogPanelBlock<'a> {
    type Target = Block<'a>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// The size of the log-panel in log-lines
pub const LINE_COUNT: usize = 20;

impl Default for LogPanelBlock<'_> {
    fn default() -> Self {
        LogPanelBlock {
            inner: Block::bordered().title("log-panel"),
            log_panel_lines: LINE_COUNT,
        }
    }
}

impl<'a> Widget for &LogPanelBlock<'a> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let LogPanelBlock { inner, .. } = self;
        inner.render(area, buf);
    }
}
impl<'a> Widget for LogPanelBlock<'a> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        <&LogPanelBlock<'a> as Widget>::render(&self, area, buf);
    }
}

trait ImplementationSpecificBlock<'a>: Deref<Target = Block<'a>> {
    fn get_rect(&self, frame: &mut Frame<'_>) -> Rect;
    fn get_enclosed_area_in(&self, frame: &mut Frame<'_>) -> Rect {
        self.deref().inner(self.get_rect(frame))
    }
}

impl<'a> ImplementationSpecificBlock<'a> for LogPanelBlock<'a> {
    fn get_rect(&self, frame: &mut Frame<'_>) -> Rect {
        crate::get_ui_rects(frame, self.log_panel_lines)[RECT_INDEX]
    }
}

pub const RECT_INDEX: usize = 1;
/// Returns the rectangle enclosed in the block
fn get_rect(frame: &mut Frame, log_panel_lines: usize) -> Rect {
    get_rect_from_area(crate::get_ui_rects(frame, log_panel_lines)[RECT_INDEX])
}
fn get_rect_from_area(log_panel_area: Rect) -> Rect {
    get_block().inner(log_panel_area)
}

trait ImplementationSpecificParagraph<'text, 'block>: Deref<Target = Paragraph<'text>> {
    fn get_block(&self) -> Option<&dyn ImplementationSpecificBlock<'block>> {
        None
    }
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let paragraph = self.deref().clone();
        let area = if let Some(block) = self.get_block() {
            let inner_block = block.deref();
            inner_block.render(area, buf);
            inner_block.inner(area).intersection(area)
        } else {
            area
        };
        // let (paragraph, inner_area) = if let Some(block) = self.get_block() {
        //     let inner_block = block.deref();
        //     let inner_area = inner_block.inner(area);
        //     (paragraph.block(inner_block.clone()), inner_area)
        // } else {
        //     (paragraph, area)
        // };
        // let paragraph = paragraph.scroll((2, 0));
        paragraph.render(area, buf);
    }
}

struct LogPanelParagraph<'text, 'block> {
    inner: Paragraph<'text>,
    block: LogPanelBlock<'block>,
}

impl<'text> Deref for LogPanelParagraph<'text, '_> {
    type Target = Paragraph<'text>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<'a> LogPanelParagraph<'a, 'static> {
    pub fn new<T: Into<Text<'a>>>(text: T) -> LogPanelParagraph<'a, 'static> {
        LogPanelParagraph {
            inner: Paragraph::new(text).scroll((2, 0)),
            block: LogPanelBlock::default(),
        }
    }
}

impl<'text, 'block> ImplementationSpecificParagraph<'text, 'block>
    for LogPanelParagraph<'text, 'block>
{
    fn get_block(&self) -> Option<&dyn ImplementationSpecificBlock<'block>> {
        Some(&self.block)
    }
}

// fn get_log_panel_rect(frame: &mut Frame, log_panel_lines: usize) -> Rect {
//     log_panel::get_block().inner(crate::get_ui_rects(frame, log_panel_lines)[RECT_INDEX])
// }
impl Widget for LogPanelWidget {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        // use ratatui::widgets::Paragraph;
        let paragraph = LogPanelParagraph::new(self.log_lines);
        paragraph.render(area, buf);
        // let border_block = LogPanelBlock::default();
        // let text_area = border_block.get_enclosed_area_in(area);
        // border_block.render(area, buf);

        // Paragraph::new(self.log_lines)
        //     .block(get_block())
        //     .render(area, buf)
    }
}
/// Just intercepts the messages and forwards them to the frontend bits
#[derive(Debug)]
struct SenderLayer<T> {
    message_tx: mpsc::Sender<T>,
}

impl<S> Layer<S> for SenderLayer<LogEvent>
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
pub struct LogEvent {
    field: Field,
    data: VisitData,
    metadata: Option<VisitMetadata>,
}

impl core::fmt::Display for LogEvent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let LogEvent {
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

impl From<LogEvent> for Line<'static> {
    fn from(value: LogEvent) -> Self {
        <&LogEvent as Into<Line<'static>>>::into(&value)
    }
}

impl From<&LogEvent> for Line<'static> {
    fn from(
        LogEvent {
            field,
            data,
            metadata,
        }: &LogEvent,
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

impl LogEvent {
    pub fn new<'p, P: ?Sized>(
        field: &Field,
        metadata: Option<&tracing::Metadata<'static>>,
        payload_data: &'p P,
    ) -> LogEvent
    where
        VisitData: From<&'p P>,
    {
        LogEvent {
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

impl SendingVisitor<'_, LogEvent> {
    fn send_payload<'p, P: ?Sized>(&self, field: &Field, payload_data: &'p P)
    where
        VisitData: From<&'p P>,
    {
        match self
            .message_tx
            .send(LogEvent::new(field, self.metadata, payload_data))
        {
            Ok(_) => {}
            Err(e) => eprintln!("Failed to send event from sending-visitor: {e}"),
        }
    }
}

impl Visit for SendingVisitor<'_, LogEvent> {
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
pub fn setup_tracing_subscriber() -> mpsc::Receiver<LogEvent> {
    let (message_tx, message_rx) = mpsc::channel();

    tracing_subscriber::registry()
        .with(SenderLayer { message_tx })
        .init();

    message_rx
}
