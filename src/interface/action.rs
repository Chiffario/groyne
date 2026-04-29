use std::rc::Rc;

use crossterm::event::KeyEvent;
use ratatui_hypertile::{HypertileAction, KeyCode};
use serde::{Deserialize, Serialize};
use twitch_api::types::Nickname;

use crate::{HelixRequest, HelixResponse, interface::components::chat::ChatMessage};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Tick,
    Render,
    Resize(u16, u16),
    Suspend,
    Resume,
    Quit,
    ClearScreen,
    Error(String),
    Help,
    Connected(Nickname),
    ChatMessage(ChatMessage),
    HelixResponse(HelixResponse),
    KeyEvent(KeyEvent),
    TilingAction(HypertileAction),
}
