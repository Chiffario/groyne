use apply::Also;
use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Direction, Layout, Spacing},
    symbols::merge::MergeStrategy,
    text::Span,
    widgets::{Block, Borders, Paragraph, Widget},
};
use ratatui_textarea::TextArea;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    interface::{action::Action, component::Component},
    tui::{Event, TerminalEvent},
};

pub struct Fetcher {
    input: TextArea<'static>,
    output: String,
}

impl Fetcher {
    pub fn new() -> Self {
        Self {
            input: TextArea::default().also(|b| b.set_block(Block::default())),
            output: String::new(),
        }
    }
}
impl Component for Fetcher {
    fn draw(
        &mut self,
        frame: &mut ratatui::Frame,
        area: ratatui::prelude::Rect,
    ) -> color_eyre::eyre::Result<()> {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .spacing(Spacing::Overlap(1))
            .split(area);

        let top = Paragraph::new(self.output.clone()).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .merge_borders(MergeStrategy::Exact),
        );

        self.input.set_block(
            Block::default()
                .borders(Borders::TOP)
                .merge_borders(MergeStrategy::Exact),
        );

        frame.render_widget(top, chunks[0]);
        frame.render_widget(&self.input, chunks[1]);
        Ok(())
    }

    fn update(
        &mut self,
        action: crate::interface::action::Action,
    ) -> color_eyre::Result<Option<crate::interface::action::Action>> {
        match action {
            Action::Tick => {}
            Action::Render => {}
            Action::KeyEvent(k) => return self.handle_key_event(k),
            _ => {}
        };
        Ok(None)
    }

    fn name(&self) -> Span {
        Span::from("Fetcher")
    }

    fn register_action_handler(
        &mut self,
        tx: tokio::sync::mpsc::UnboundedSender<Action>,
    ) -> color_eyre::Result<()> {
        let _ = tx; // to appease clippy
        Ok(())
    }

    fn handle_key_event(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> color_eyre::Result<Option<Action>> {
        self.input.input(key);
        Ok(None)
    }

    fn handle_mouse_event(
        &mut self,
        mouse: crossterm::event::MouseEvent,
    ) -> color_eyre::Result<Option<Action>> {
        let _ = mouse; // to appease clippy
        Ok(None)
    }
}
