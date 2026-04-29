use color_eyre::eyre;
use tokio::sync::mpsc::UnboundedSender;
use tracing::info_span;
use twitch_api::twitch_oauth2::UserToken;

use crate::{
    api::eventsub::{InitialChatWebsocketConnection, WebsocketConnection},
    interface::{action::Action, components::chat::ChatMessage},
};

mod eventsub;
pub async fn run(token: UserToken, osu_tx: UnboundedSender<Action>) -> eyre::Result<()> {
    let mut initial_conn = InitialChatWebsocketConnection::new(token).await;
    tracing::debug!("Created initial websocket connection");

    let span = info_span!("connection creation");
    let new_conn = loop {
        match initial_conn.receive_message().await {
            Ok(Some(frame)) => {
                tracing::trace!(parent: &span, "Creating a full client");
                let client = initial_conn.create_full_client(frame, osu_tx.clone()).await;
                break client;
            }
            Ok(None) => {
                tracing::warn!(parent: &span, "Failed to create a new connection, retrying");
            }
            Err(e) => {
                tracing::error!(parent: &span, "Error receiving message: {e}");
                break Err(e);
            }
        }
    }?;

    let mut conn_clone = new_conn;
    tracing::trace!("Starting a receive task");
    let message_task = tokio::spawn(async move {
        tracing::trace!("Starting a message reception loop");
        conn_clone
            .subscribe_to_channels_initially()
            .await
            .inspect_err(|e| tracing::warn!("Error when subscribing: {e}"))
            .unwrap();
        loop {
            let message = conn_clone.receive_message().await;
            match message {
                Ok(Some(str)) => {
                    conn_clone.handle_message(str).await.unwrap();
                }
                Ok(None) => {
                    tracing::trace!("Received an empty message");
                }
                Err(e) => {
                    tracing::warn!(?e, "Received an error");
                    break;
                }
            }
        }
    });

    let _ = tokio::join!(message_task);
    tracing::trace!("Returning from twitch tasks");
    Ok(())
}
