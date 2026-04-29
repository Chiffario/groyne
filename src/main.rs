mod api;
mod configuration;
pub mod interface;
pub mod tui;

use apply::Also;
use color_eyre::Result;
use futures::{SinkExt, StreamExt};
use interface::components::chat::Chat;
use ratatui::crossterm::event::{Event, KeyCode, poll};
use ratatui::layout::{Constraint, Layout, Margin, Rect, Spacing};
use ratatui::symbols::merge::MergeStrategy;
use ratatui::widgets::{ListState, Paragraph, Row, Table};
use ratatui::{DefaultTerminal, Frame};
use ratatui::{
    buffer::Buffer,
    layout::Direction,
    style::{Color, Style},
    widgets::{Block, Borders, Widget},
};
use ratatui_hypertile::{
    Hypertile, HypertileAction, HypertileWidget, MoveScope, PaneId, PaneSnapshot, Towards,
};
use ratatui_textarea::TextArea;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::Path;
use std::str::FromStr;
use std::{io, time::Duration};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tracing::Instrument;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use twitch_api::HelixClient;
use twitch_api::helix::channels::{
    ChannelInformation, ModifyChannelInformation, ModifyChannelInformationBody,
    modify_channel_information,
};
use twitch_api::twitch_oauth2::{AccessToken, UserToken};
use twitch_api::types::{CategoryId, UserId};

use crate::interface::action::Action;
use crate::interface::app::App;
use crate::tui::Tui;

const REDIRECT_URL: &str = "http://localhost:11111/test";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum HelixRequest {
    GetChannel {
        username: String,
    },
    UpdateChannel {
        id: UserId,
        #[serde(borrow)]
        body: ModifyChannelInformationBody<'static>,
    },
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
enum HelixResponse {
    Channel(ChannelInformation),
    Debug(String),
}

const USER_ID: &str = "129898402";

fn setup_env() -> Result<()> {
    let log_dir = Path::new("/tmp/groyne.log");
    let file = std::fs::File::create(log_dir)?;

    let log_filter =
        std::env::var("RUST_LOG").unwrap_or_else(|_| format!("{}=info", env!("CARGO_CRATE_NAME")));

    let layer = tracing_subscriber::fmt::layer()
        .with_line_number(true)
        .with_ansi(true)
        .with_writer(file)
        .with_filter(tracing_subscriber::filter::EnvFilter::builder().parse_lossy(log_filter));

    tracing_subscriber::registry().with(layer).init();

    Ok(())
}

async fn init_helix_thread(
    token: String,
    mut helix_rx: UnboundedReceiver<HelixRequest>,
    helix_resp_tx: UnboundedSender<Action>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let helix_client: HelixClient<reqwest::Client> =
            HelixClient::with_client(reqwest::Client::new());
        let access_token = AccessToken::from_str(&token).unwrap();
        let client_token = UserToken::from_token(&helix_client, access_token)
            .await
            .unwrap();
        tracing::debug!(login = client_token.login.as_str(), "Initialized a token");

        let client = helix_client.clone();
        tracing::debug!("Starting a Helix thread loop");
        // println!("helix thread: Initialized tokens");
        while let Some(message) = helix_rx.recv().await {
            // println!("helix thread: receiving Message: {:?}", message);
            tracing::debug!(message = ?message, "Received a message");
            let resp = match message {
                HelixRequest::GetChannel { username } => HelixResponse::Channel(
                    client
                        .get_channel_from_login(&username, &client_token)
                        .await
                        .unwrap()
                        .unwrap(),
                ),
                HelixRequest::UpdateChannel { id, body } => {
                    let request =
                        modify_channel_information::ModifyChannelInformationRequest::broadcaster_id(
                            id.clone(),
                        );
                    let response = client
                        .req_patch(request, body, &client_token)
                        .await
                        .unwrap();
                    if let ModifyChannelInformation::Success = response.data {
                        HelixResponse::Channel(
                            client
                                .get_channel_from_id(&id, &client_token)
                                .await
                                .unwrap()
                                .unwrap(),
                        )
                    } else {
                        panic!()
                    }
                }
            };
            helix_resp_tx.send(Action::HelixResponse(resp)).unwrap();
        }
    })
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    setup_env()?;
    let config = configuration::read_configuration();
    tracing::debug!("Initialized configuration");

    let (helix_tx, helix_rx) = mpsc::unbounded_channel();
    let (helix_resp_tx, helix_resp_rx) = mpsc::unbounded_channel();
    let mut app = App::new(1., 5., helix_resp_tx.clone(), helix_resp_rx)?;
    let app_fut = app.run();
    tracing::trace!("Initialized Helix channels");
    let client_token = config.twitch.access_token.clone();

    let helix_resp_tx_clone = helix_resp_tx.clone();
    let _handle = init_helix_thread(
        config.twitch.access_token.clone(),
        helix_rx,
        helix_resp_tx_clone.clone(),
    )
    .await;

    let _handle2 = tokio::spawn(async move {
        let helix_client: HelixClient<reqwest::Client> =
            HelixClient::with_client(reqwest::Client::new());
        let access_token = AccessToken::from_str(&client_token).unwrap();
        let client_token = UserToken::from_token(&helix_client, access_token)
            .await
            .unwrap();
        api::run(client_token, helix_resp_tx.clone()).await
    });

    tracing::debug!("Starting app up");
    tokio::select! {
        fin = _handle => { tracing::debug!("Finishing Helix execution"); },
        fin = _handle2 => { tracing::debug!("Finishing EventSub execution"); },
        app = app_fut => { tracing::debug!("Killing TUI"); }
    };
    Ok(())
}
