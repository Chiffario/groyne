use std::str::FromStr;

use apply::{Also, Apply};
use jiff::{Timestamp, Unit, tz::TimeZone};
use ratatui::{
    layout::Margin,
    prelude::{Buffer, Rect},
    style::{Color, Style},
    text::{Line, Span, Text, ToSpan},
    widgets::{Block, List, ListState, StatefulWidget, Widget},
};
use ringbuffer::{AllocRingBuffer, RingBuffer};
use serde::{Deserialize, Serialize};

use crate::{
    configuration::{ChatConfig, Configuration},
    interface::{action::Action, component::Component},
};

pub struct Chat {
    state: ChatState,
    config: ChatConfig,
}

impl Chat {
    pub fn new() -> Self {
        Self {
            state: ChatState::new(),
            config: ChatConfig::default(),
        }
    }

    fn format_message(&self, value: &ChatMessage) -> Text {
        let mut parts: Vec<Span> = vec![];
        if self.config.timestamp {
            let timestamp: jiff::civil::Time = value.timestamp.to_zoned(TimeZone::system()).into();
            let span: Span = Span::from(
                timestamp
                    .round(Unit::Second)
                    .unwrap()
                    .to_string()
                    .also(|t| t.push(' ')),
            )
            .style(Style::new().gray());
            parts.push(span);
        }
        let mut username = Span::from(value.username.clone());
        if self.config.use_color {
            username = username.style(Style::new().fg(Color::from_str(&value.color).unwrap()));
        }
        parts.push(username);
        let message = Span::from(format!(": {}", value.text));
        parts.push(message);
        Text::from(Line::from_iter(parts))
    }
}

pub struct ChatState {
    chat_buffer: AllocRingBuffer<ChatMessage>,
    channel: Option<String>,
}

impl ChatState {
    pub fn new() -> Self {
        let chat_buffer = AllocRingBuffer::new(50);
        Self {
            chat_buffer,
            channel: None,
        }
    }

    pub fn push(&mut self, message: ChatMessage) {
        self.chat_buffer.enqueue(message);
    }

    pub fn messages(&self) -> impl Iterator<Item = &ChatMessage> {
        self.chat_buffer.iter()
    }
}

impl Component for Chat {
    fn draw(&mut self, frame: &mut ratatui::Frame, area: Rect) -> color_eyre::Result<()> {
        let list = List::from(
            self.state
                .chat_buffer
                .iter()
                .map(|m| self.format_message(m))
                .collect(),
        );

        // one col inner padding for cleanliness, idk if necessary tbqh
        frame.render_widget(list, area.inner(Margin::new(self.config.margin as u16, 0)));
        Ok(())
    }

    fn update(
        &mut self,
        action: crate::interface::action::Action,
    ) -> color_eyre::Result<Option<crate::interface::action::Action>> {
        let span = tracing::debug_span!("Chat update");
        match action {
            Action::Tick => {}
            Action::Render => {}
            Action::Connected(str) => {
                tracing::debug!(parent: &span, username = %str, "Connected to chat");
                self.state.channel = Some(str.to_string());
            }
            Action::ChatMessage(chat_message) => {
                tracing::debug!(parent: &span, message = ?chat_message, "Updating chat with");
                self.state.push(chat_message)
            }
            action => {
                tracing::debug!(parent: &span, "received message {action:?}");
            }
        }
        Ok(None)
    }

    fn register_config_handler(&mut self, config: Configuration) -> color_eyre::Result<()> {
        tracing::debug!(name = self.name().to_string(), "Registered config handler");
        self.config = config.chat.clone();
        Ok(())
    }

    fn name(&self) -> Span {
        match &self.state.channel {
            Some(name) => Span::from(format!("Chat: [{}]", name)),
            None => Span::from("Connecting..."),
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct ChatMessage {
    pub username: String,
    pub(crate) text: String,
    pub color: String,
    pub timestamp: Timestamp,
}
