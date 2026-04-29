use std::collections::HashMap;

use color_eyre::eyre::Context;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::Rect;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tracing::{Instrument, debug, info};

use crate::{
    configuration::{Configuration, KeyBindings, read_configuration},
    interface::{
        action::Action,
        component::Component,
        components::{chat::Chat, fetcher::Fetcher, home::Home, tiler::Tiler},
    },
    tui::{Event, TerminalEvent, Tui},
};

pub struct App {
    config: Configuration,
    tick_rate: f64,
    frame_rate: f64,
    components: Vec<Box<dyn Component>>,
    should_quit: bool,
    should_suspend: bool,
    last_tick_key_events: Vec<KeyEvent>,
    action_tx: mpsc::UnboundedSender<Action>,
    action_rx: mpsc::UnboundedReceiver<Action>,
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Home,
}

impl App {
    pub fn new(
        tick_rate: f64,
        frame_rate: f64,
        action_tx: UnboundedSender<Action>,
        action_rx: UnboundedReceiver<Action>,
    ) -> color_eyre::Result<Self> {
        Ok(Self {
            tick_rate,
            frame_rate,
            components: vec![Box::new(Tiler::new(action_tx.clone()))],
            should_quit: false,
            should_suspend: false,
            config: read_configuration(),
            last_tick_key_events: Vec::new(),
            action_tx,
            action_rx,
        })
    }

    #[tracing::instrument(skip(self))]
    pub async fn run(&mut self) -> color_eyre::Result<()> {
        let mut tui = Tui::new()?
            // .mouse(true) // uncomment this line to enable mouse support
            .tick_rate(self.tick_rate)
            .frame_rate(self.frame_rate);
        tracing::debug!(
            "Starting a TUI with tick rate of {} and fps of {}",
            self.tick_rate,
            self.frame_rate
        );
        tui.enter()?;

        tracing::debug!("Registering {} components", self.components.len());
        for component in self.components.iter_mut() {
            component.register_action_handler(self.action_tx.clone())?;
        }
        for component in self.components.iter_mut() {
            component.register_config_handler(self.config.clone())?;
        }
        for component in self.components.iter_mut() {
            component.init(tui.size()?)?;
        }
        tracing::debug!("Finished registering {} components", self.components.len());

        let action_tx = self.action_tx.clone();
        loop {
            self.handle_events(&mut tui).await?;
            self.handle_actions(&mut tui)?;
            if self.should_suspend {
                tui.suspend()?;
                tracing::debug!("Suspending application");
                action_tx.send(Action::Resume)?;
                action_tx.send(Action::ClearScreen)?;
                // tui.mouse(true);
                tui.enter()?;
            } else if self.should_quit {
                tracing::debug!("Quitting application");
                tui.stop()?;
                break;
            }
        }
        tui.exit()?;
        Ok(())
    }

    async fn handle_events(&mut self, tui: &mut Tui) -> color_eyre::Result<()> {
        let Some(event) = tui.next_event().await else {
            return Ok(());
        };
        tracing::trace!(event = ?event, "Received event");
        let action_tx = self.action_tx.clone();
        match event {
            Event::Quit => action_tx.send(Action::Quit)?,
            Event::Tick => action_tx.send(Action::Tick)?,
            Event::Render => action_tx.send(Action::Render)?,
            Event::CrosstermEvent(TerminalEvent::Resize(x, y)) => {
                action_tx.send(Action::Resize(x, y))?
            }
            Event::CrosstermEvent(TerminalEvent::Key(key)) => self.handle_key_event(key)?,
            _ => {}
        }
        for component in self.components.iter_mut() {
            if let Some(action) = component.handle_events(Some(event.clone()))? {
                action_tx.send(action)?;
            }
        }
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> color_eyre::Result<()> {
        let action_tx = self.action_tx.clone();

        tracing::trace!(key_event = ?key, "Received key event");
        match key.code {
            KeyCode::Esc => action_tx
                .send(Action::Quit)
                .wrap_err("Failed to send action"),
            KeyCode::Tab => action_tx
                .send(Action::TilingAction(
                    ratatui_hypertile::HypertileAction::FocusNext,
                ))
                .wrap_err("Failed to send action"),
            _ => {
                action_tx.send(Action::KeyEvent(key)).unwrap();
                Ok(())
            }
        }
    }

    fn handle_actions(&mut self, tui: &mut Tui) -> color_eyre::Result<()> {
        while let Ok(action) = self.action_rx.try_recv() {
            if action != Action::Tick && action != Action::Render {
                debug!("{action:?}");
            }
            tracing::trace!(action = ?action, "Handling action");
            match action {
                Action::Tick => {
                    self.last_tick_key_events.drain(..);
                }
                Action::Quit => self.should_quit = true,
                Action::Suspend => self.should_suspend = true,
                Action::Resume => self.should_suspend = false,
                Action::ClearScreen => tui.terminal.clear()?,
                Action::Resize(w, h) => self.handle_resize(tui, w, h)?,
                Action::Render => self.render(tui)?,
                _ => {}
            }
            for component in self.components.iter_mut() {
                if let Some(action) = component.update(action.clone())? {
                    tracing::debug!(action = ?action, "Propagating action");
                    self.action_tx.send(action)?
                };
            }
        }
        Ok(())
    }

    fn handle_resize(&mut self, tui: &mut Tui, w: u16, h: u16) -> color_eyre::Result<()> {
        tracing::debug!(new_size = ?(w, h), "Resizing to");
        tui.resize(Rect::new(0, 0, w, h))?;
        self.render(tui)?;
        Ok(())
    }

    fn render(&mut self, tui: &mut Tui) -> color_eyre::Result<()> {
        tui.draw(|frame| {
            for component in self.components.iter_mut() {
                if let Err(err) = component.draw(frame, frame.area()) {
                    tracing::warn!("Rendering error: {err:?}");
                    let _ = self
                        .action_tx
                        .send(Action::Error(format!("Failed to draw: {:?}", err)));
                }
            }
        })?;
        Ok(())
    }
}
