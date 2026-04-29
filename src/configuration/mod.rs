use std::{collections::HashMap, fs::File, io::Read};

use apply::Also;
use crossterm::event::KeyEvent;
use serde::{Deserialize, Serialize};

use crate::interface::{action::Action, app::Mode};

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Configuration {
    pub twitch: Twitch,
    #[serde(default)]
    pub chat: ChatConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Twitch {
    pub username: String,
    pub access_token: String,
}
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ChatConfig {
    pub timestamp: bool,
    pub use_color: bool,
    pub margin: u8,
}

impl Configuration {
    pub fn new(username: String, access_token: String) -> Self {
        Self {
            twitch: Twitch {
                username,
                access_token,
            },
            chat: ChatConfig::default(),
        }
    }
}

pub fn read_configuration() -> Configuration {
    let base_dirs = directories::BaseDirs::new().unwrap();
    let config_dir = base_dirs
        .config_dir()
        .to_path_buf()
        .also(|c| c.push("groyne"))
        .also(|c| c.push("config.toml"));

    // note: might wanna try memmap2 for fun here
    let mut buffer = String::new();
    let mut file = File::open(config_dir).unwrap();

    file.read_to_string(&mut buffer).unwrap();
    toml::from_str(&buffer).unwrap()
}

#[derive(Clone, Debug, Default)]
pub struct KeyBindings(pub HashMap<Mode, HashMap<Vec<KeyEvent>, Box<Action>>>);
