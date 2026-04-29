use color_eyre::eyre::{self, Context, Error, Result, bail};
use crossbeam_channel::Sender;
use futures::{StreamExt, stream::SplitStream};
use jiff::Timestamp;
use reqwest::Client;
use std::{panic, time::Instant};
use tokio::{
    sync::{Mutex, mpsc::UnboundedSender},
    time::Duration,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message as WsMessage, protocol::WebSocketConfig},
};
use toml::value::Time;
use twitch_api::{
    eventsub::{
        self, Event, EventSubscription, Message, Transport,
        channel::{ChannelChatMessageV1, ChannelChatMessageV1Payload},
        event::websocket::{EventsubWebsocketData, WelcomePayload},
    },
    helix::{HelixClient, eventsub::CreateEventSubSubscription},
    twitch_oauth2::{TwitchToken, UserToken},
    types::UserId,
};

use crate::interface::{action::Action, components::chat::ChatMessage};

type TwitchClient<'a> = HelixClient<'a, Client>;

/// Connect to the websocket and return the stream
async fn connect(
    request: impl AsRef<str>,
) -> Result<SplitStream<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>, Error> {
    let config = Some(WebSocketConfig {
        max_message_size: Some(64 << 20), // 64 MiB
        max_frame_size: Some(16 << 20),   // 16 MiB
        accept_unmasked_frames: false,
        ..WebSocketConfig::default()
    });
    let socket = tokio_tungstenite::connect_async_with_config(request.as_ref(), config, false)
        .await
        .context("Can't connect")?
        .0
        .split()
        .1;

    tracing::debug!(url = request.as_ref(), "Created a websocket");
    Ok(socket)
}

/// Check expiration time for UserToken and refresh if necessary
#[tracing::instrument(skip_all)]
async fn refresh_if_expired(token: &Mutex<UserToken>, helix_client: &HelixClient<'_, Client>) {
    let mut lock = token.lock().await;

    if lock.expires_in() >= Duration::from_secs(60) {
        tracing::trace!(expires_in = ?lock.expires_in(), "Token has not expired yet");
        return;
    }

    let client = helix_client.get_client();
    let _ = lock.refresh_token(client).await;
    tracing::debug!("Refreshed user token");

    drop(lock);
}

pub fn get_bot_id() -> UserId {
    // this *might* be worth moving to compile time but idk
    UserId::from_static("129898402")
}

pub trait WebsocketConnection {
    async fn receive_message(&mut self) -> Result<Option<String>>;
}

pub struct InitialChatWebsocketConnection<'a> {
    pub token: Mutex<UserToken>,
    pub client: HelixClient<'a, Client>,
    socket: SplitStream<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>,
}

pub struct ChatWebsocketConnection<'a> {
    /// UserToken behind a Mutex to avoid task overlap issues
    token: Mutex<UserToken>,
    client: HelixClient<'a, reqwest::Client>,
    socket: SplitStream<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>,
    /// EventSub session ID, mostly necessary for adding subs
    session_id: String,
    request_tx: UnboundedSender<Action>,
}

const BASE_WEBSOCKET_URL: &str = "wss://eventsub.wss.twitch.tv/ws";
impl<'a> InitialChatWebsocketConnection<'a> {
    #[tracing::instrument(skip_all)]
    pub async fn new(token: UserToken) -> Self {
        // if the initial connection fails, the entire thing is likely unrecoverable
        let socket = connect("wss://eventsub.wss.twitch.tv/ws").await.unwrap();
        tracing::trace!("Connected to base EventSub at {BASE_WEBSOCKET_URL}");
        let token = Mutex::new(token);
        let client: TwitchClient = HelixClient::new();

        tracing::debug!("Successfully created a Twitch API client");
        Self {
            token,
            client,
            socket,
        }
    }

    /// Creates a ChatWebsocketConnection on successful Welcome message
    pub async fn create_full_client(
        self,
        frame: String,
        osu_tx: UnboundedSender<Action>,
    ) -> Result<ChatWebsocketConnection<'a>> {
        let event_data =
            Event::parse_websocket(&frame).wrap_err("Failed to parse a Websocket frame")?;
        tracing::trace!(?event_data, "Handling message for full client");
        match event_data {
            EventsubWebsocketData::Welcome {
                payload: WelcomePayload { session },
                ..
            } => {
                tracing::debug!("Received a Welcome message, creating a new client");

                let token = Mutex::new(self.token.into_inner());
                let client: HelixClient<'_, Client> = HelixClient::new();

                Ok(ChatWebsocketConnection {
                    token,
                    client,
                    socket: self.socket,
                    session_id: session.id.to_string(),
                    request_tx: osu_tx,
                })
            }
            EventsubWebsocketData::Keepalive {
                metadata,
                payload: _,
            } => {
                tracing::trace!(
                    ?metadata,
                    "Received a Keepalive message before init is done"
                );
                bail!("Received a Keepalive");
            }
            _ => {
                tracing::error!(?event_data, "Received an unexpected message, bailing");
                bail!("Received an unexpected message");
            }
        }
    }
}

impl<'a> ChatWebsocketConnection<'a> {
    #[tracing::instrument(skip_all)]
    pub async fn handle_message(&mut self, frame: String) -> Result<()> {
        let event = Event::parse_websocket(&frame).wrap_err("Failed to parse a Websocket frame")?;
        match event {
            EventsubWebsocketData::Welcome {
                metadata: _,
                payload: _,
            } => {
                tracing::error!("Received an unexpected Welcome message");
                Ok(())
            }
            EventsubWebsocketData::Keepalive {
                metadata: _,
                payload: _,
            } => {
                tracing::trace!("Received a KeepAlive heartbeat");
                Ok(())
            }
            EventsubWebsocketData::Notification { metadata, payload } => {
                tracing::debug!(
                    notification_type = metadata.subscription_type.to_str(),
                    "Received a notification"
                );
                self.handle_notification(payload).await
            }
            EventsubWebsocketData::Revocation {
                metadata: _,
                payload: _,
            } => {
                todo!("I'm not sure yet how to handle revocations")
            }
            EventsubWebsocketData::Reconnect {
                metadata: _,
                payload,
            } => {
                tracing::trace!("Received a reconnect event");
                self.socket = connect(payload.session.reconnect_url.unwrap().as_ref())
                    .await
                    .wrap_err("Failed to reconnect to EventSub")?;
                tracing::trace!("Reconnected to EventSub");
                Ok(())
            }
            _ => todo!(),
        }
    }

    /// Handles a notification event. Until further notice, only needs to handle channel.chat.message
    async fn handle_notification(&mut self, event: Event) -> Result<()> {
        match event {
            Event::ChannelChatMessageV1(eventsub::Payload { message, .. }) => {
                tracing::trace!("Message is a channel.chat.message");

                match message {
                    Message::VerificationRequest(_) => unreachable!(
                        "Verification requests shouldn't come through for WebSocket connections"
                    ),
                    Message::Revocation() => bail!("Unexpected subscription revocation"),
                    Message::Notification(e) => self.process_chat_message(e).await,
                    _ => todo!(),
                }
            }
            _ => {
                tracing::error!("Unexpected message type, bailing");
                panic!("Unexpected message type");
            }
        }
    }

    /// Processes message data. Primarily parses and sends a beatmap if found
    async fn process_chat_message(&mut self, payload: ChannelChatMessageV1Payload) -> Result<()> {
        let request = self.construct_message_from_payload(payload);
        match request {
            Ok(Some(request)) => {
                tracing::debug!(?request, "Constructed a valid request");
                self.request_tx
                    .send(Action::ChatMessage(request))
                    .wrap_err("Failed to process chat message")
            }
            Ok(None) => {
                tracing::trace!("Not a valid request");
                Ok(())
            }
            Err(err) => {
                tracing::error!(?err, "Failed to parse a message");
                Err(err)
            }
        }
    }

    /// Attempt to parse a beatmap ID from a Twitch message
    fn construct_message_from_payload(
        &self,
        payload: ChannelChatMessageV1Payload,
    ) -> Result<Option<ChatMessage>> {
        tracing::trace!(
            from = %payload.chatter_user_name,
            to = %payload.broadcaster_user_name,
            message = payload.message.text,
            "Parsing a message");

        let timestamp = Timestamp::now();

        Ok(Some(ChatMessage {
            username: payload.chatter_user_name.to_string(),
            text: payload.message.text,
            color: payload.color.take(),
            timestamp,
        }))
    }

    /// Subscribe to a list of channels obtained from the API. Placeholder as this should be handler better
    pub async fn subscribe_to_channels_initially(&mut self) -> Result<()> {
        let id = UserId::from_static("129898402");
        let subscription_result = self.subscribe_to_channel(&id).await?;
        let login = self
            .client
            .get_user_from_id(&id, &self.token.lock().await.clone())
            .await
            .unwrap()
            .unwrap()
            .login;
        tracing::debug!(
            streamer_id = ?subscription_result.condition.broadcaster_user_id,
            "Created new subscription"
        );
        self.request_tx.send(Action::Connected(login)).unwrap();
        Ok(())
    }

    /// Create an EventSub subscription to a channel
    async fn subscribe_to_channel(
        &mut self,
        channel_id: &UserId,
    ) -> Result<CreateEventSubSubscription<ChannelChatMessageV1>> {
        let token = self.token.lock().await.clone();
        let result = self
            .client
            .create_eventsub_subscription(
                ChannelChatMessageV1::new(channel_id.to_owned(), get_bot_id()),
                Transport::websocket(&self.session_id),
                &token,
            )
            .await
            .inspect_err(|e| tracing::warn!("Failed to subscribe to a channel: {e}"))?;
        // TODO: this needs to propagate user IDs
        let event_id = result.id.clone();
        tracing::debug!(user_id = ?channel_id, "Subscribed to user");

        tracing::trace!(event_id = %event_id.clone(), "New event subscription ID");
        Ok(result)
    }
}

impl<'a> WebsocketConnection for InitialChatWebsocketConnection<'a> {
    async fn receive_message(&mut self) -> Result<Option<String>> {
        let Some(message) = self.socket.next().await else {
            return Err(eyre::eyre!("websocket stream closed unexpectedly"));
        };
        match message.context("tungstenite error")? {
            WsMessage::Close(frame) => {
                let reason = frame.map(|frame| frame.reason).unwrap_or_default();
                tracing::error!("Connection closed with reason: {reason}");
                Err(eyre::eyre!(
                    "websocket stream closed unexpectedly with reason {reason}"
                ))
            }
            WsMessage::Frame(_) => unreachable!(),
            WsMessage::Ping(_) | WsMessage::Pong(_) => {
                // no need to do anything as tungstenite automatically handles pings for you
                // but refresh the token just in case
                refresh_if_expired(&self.token, &self.client).await;
                Ok(None)
            }
            WsMessage::Binary(_) => unimplemented!(),
            WsMessage::Text(payload) => {
                tracing::trace!(%payload, "Received message");
                Ok(Some(payload))
            }
        }
    }
}
impl<'a> WebsocketConnection for ChatWebsocketConnection<'a> {
    async fn receive_message(&mut self) -> Result<Option<String>> {
        let Some(message) = self.socket.next().await else {
            return Err(eyre::eyre!("websocket stream closed unexpectedly"));
        };
        match message.context("tungstenite error")? {
            WsMessage::Close(frame) => {
                let reason = frame.map(|frame| frame.reason).unwrap_or_default();
                Err(eyre::eyre!(
                    "websocket stream closed unexpectedly with reason {reason}"
                ))
            }
            WsMessage::Frame(_) => unreachable!(),
            WsMessage::Ping(_) | WsMessage::Pong(_) => {
                // no need to do anything as tungstenite automatically handles pings for you
                // but refresh the token just in case
                refresh_if_expired(&self.token, &self.client).await;
                Ok(None)
            }
            WsMessage::Binary(_) => unimplemented!(),
            WsMessage::Text(payload) => Ok(Some(payload)),
        }
    }
}
