use ratatui::text::Span;
use twitch_api::helix::channels::ChannelInformation;

use crate::interface::component::Component;

struct PartialChannelInformation {
    title: String,
    description: String,
    game: String,
}

impl From<ChannelInformation> for PartialChannelInformation {
    fn from(value: ChannelInformation) -> Self {
        Self {
            title: value.title,
            description: value.description,
            game: value.game_name.take(),
        }
    }
}

struct Metadata {
    data: ChannelInformation,
}

impl Component for Metadata {
    fn name(&self) -> Span {
        Span::from("Stream info")
    }

    fn draw(
        &mut self,
        frame: &mut ratatui::Frame,
        area: ratatui::prelude::Rect,
    ) -> color_eyre::Result<()> {
        todo!()
    }
}
