use ratatui::{
    Frame,
    layout::{Rect, Size},
    style::Style,
    symbols::merge::MergeStrategy,
    text::Span,
    widgets::{Block, Widget},
};
use ratatui_hypertile::{Hypertile, HypertileWidget, PaneId};
use std::collections::HashMap;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    configuration::{Configuration, read_configuration},
    interface::{
        action::Action,
        component::Component,
        components::{chat::Chat, fetcher::Fetcher, home::Home},
    },
};

pub struct Tiler {
    layout: Hypertile,
    action_tx: UnboundedSender<Action>,
    panes: HashMap<PaneId, Box<dyn Component>>,
    config: Configuration,
}

impl Tiler {
    pub fn new(action_tx: UnboundedSender<Action>) -> Self {
        Self {
            layout: Hypertile::new(),
            panes: HashMap::new(),
            action_tx,
            config: read_configuration(),
        }
    }

    pub fn layout(&self) -> &Hypertile {
        &self.layout
    }

    pub fn layout_mut(&mut self) -> &mut Hypertile {
        &mut self.layout
    }

    pub fn insert_pane(&mut self, component: Box<dyn Component>) -> Option<PaneId> {
        let id = if self.panes.is_empty() {
            PaneId::ROOT
        } else {
            let id = self
                .layout_mut()
                .split_focused(ratatui::layout::Direction::Horizontal)
                .unwrap();
            tracing::debug!("New pane at id {}", id);
            id
        };
        tracing::debug!(id = ?id, component = component.name().to_string(), "Inserting pane");
        self.panes.insert(id, component);
        Some(id)
    }

    pub fn remove_pane(&mut self, pane_id: PaneId) -> Option<Box<dyn Component>> {
        self.panes.remove(&pane_id)
    }

    pub fn get_focused_pane(&mut self) -> Option<&mut Box<dyn Component>> {
        // Must always be non-empty
        let id = self.layout().focused_pane().unwrap();
        self.panes.get_mut(&id)
    }
}

impl Component for Tiler {
    fn init(&mut self, area: Size) -> color_eyre::Result<()> {
        tracing::debug!(
            "Initializing tiler at {:?}",
            Rect::new(0, 0, area.width, area.height)
        );
        self.insert_pane(Box::new(Chat::new()));
        self.insert_pane(Box::new(Fetcher::new()));
        self.layout
            .compute_layout(Rect::new(0, 0, area.width, area.height));
        tracing::debug!(layout = ?self.layout().panes().iter().map(|p| p.rect));
        for component in self.panes.values_mut() {
            component.register_action_handler(self.action_tx.clone())?;
        }
        for component in self.panes.values_mut() {
            component.register_config_handler(self.config.clone())?;
        }
        for component in self.panes.values_mut() {
            component.init(area)?;
        }
        Ok(())
    }

    fn update(
        &mut self,
        action: crate::interface::action::Action,
    ) -> color_eyre::Result<Option<crate::interface::action::Action>> {
        match action {
            Action::TilingAction(hypertile_action) => {
                self.layout_mut().apply_action(hypertile_action);
            }
            Action::KeyEvent(_) => {
                let focused = self.get_focused_pane().unwrap();
                focused.update(action.clone()).unwrap();
            }
            act => {
                if act != Action::Render && act != Action::Tick {
                    tracing::debug!(action = ?act, "Propagating action");
                }
                for component in self.panes.values_mut() {
                    component.update(act.clone()).unwrap();
                }
            }
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        self.layout.compute_layout(area);
        for pane in self.layout.panes_iter() {
            if let Some(component) = self.panes.get_mut(&pane.id) {
                let mut block = Block::bordered().title(component.name());
                if pane.is_focused {
                    block = block.border_style(Style::default().blue());
                }
                let inner = block.inner(pane.rect);
                block.render(pane.rect, frame.buffer_mut());
                component.draw(frame, inner)?;
            }
        }
        Ok(())
    }

    fn name(&self) -> Span {
        String::from("Tiler").into()
    }

    fn register_config_handler(
        &mut self,
        config: crate::configuration::Configuration,
    ) -> color_eyre::Result<()> {
        for component in self.panes.values_mut() {
            component.register_config_handler(config.clone())?;
        }
        Ok(())
    }

    fn handle_events(
        &mut self,
        event: Option<crate::tui::Event>,
    ) -> color_eyre::Result<Option<Action>> {
        let action = match event {
            Some(crate::tui::Event::CrosstermEvent(crate::tui::TerminalEvent::Key(key_event))) => {
                self.handle_key_event(key_event)?
            }
            Some(crate::tui::Event::CrosstermEvent(crate::tui::TerminalEvent::Mouse(
                mouse_event,
            ))) => self.handle_mouse_event(mouse_event)?,
            _ => None,
        };
        Ok(action)
    }

    fn handle_key_event(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> color_eyre::Result<Option<Action>> {
        let _ = key; // to appease clippy
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
