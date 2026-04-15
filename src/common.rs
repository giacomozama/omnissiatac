use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use lavalink_rs::prelude::*;
use serenity::all::{CommandInteraction, Message};
use serenity::builder::{
    CreateAttachment, CreateInteractionResponse, CreateInteractionResponseMessage,
    EditInteractionResponse,
};
use serenity::model::id::{ChannelId, GuildId, UserId};
use serenity::prelude::*;
use tracing::error;

use crate::config::Config;
use crate::error::{BotError, BotResult};
use crate::llm::ChatMessage;

pub struct Lavalink;

impl TypeMapKey for Lavalink {
    type Value = LavalinkClient;
}

pub struct InactivityMap;

impl TypeMapKey for InactivityMap {
    type Value = Arc<DashMap<GuildId, Instant>>;
}

pub struct ConfigKey;

impl TypeMapKey for ConfigKey {
    type Value = Arc<tokio::sync::RwLock<Config>>;
}

pub struct ChatHistory;

impl TypeMapKey for ChatHistory {
    type Value = Arc<DashMap<ChannelId, Vec<ChatMessage>>>;
}

pub async fn get_lava_client(ctx: &Context) -> LavalinkClient {
    let data = ctx.data.read().await;
    data.get::<Lavalink>()
        .cloned()
        .expect("Lavalink client not found in TypeMap")
}

pub fn get_user_voice_channel(
    ctx: &Context,
    guild_id: GuildId,
    user_id: UserId,
) -> Option<ChannelId> {
    let guild = ctx.cache.guild(guild_id)?;
    guild
        .voice_states
        .get(&user_id)
        .and_then(|voice_state| voice_state.channel_id)
}

pub async fn ensure_joined(
    ctx: &Context,
    lava_client: &LavalinkClient,
    guild_id: GuildId,
    voice_channel_id: ChannelId,
    text_channel_id: ChannelId,
) -> BotResult<bool> {
    let manager = songbird::get(ctx)
        .await
        .expect("Songbird manager not found")
        .clone();
    let bot_id = ctx.cache.current_user().id;
    let bot_voice_channel = ctx
        .cache
        .guild(guild_id)
        .and_then(|g| g.voice_states.get(&bot_id).and_then(|vs| vs.channel_id));

    let should_join = match bot_voice_channel {
        Some(channel) => {
            channel != voice_channel_id || lava_client.get_player_context(guild_id).is_none()
        }
        None => true,
    };

    if should_join {
        match manager.join_gateway(guild_id, voice_channel_id).await {
            Ok((connection_info, _)) => {
                lava_client
                    .create_player_context_with_data::<(ChannelId, Arc<serenity::http::Http>)>(
                        guild_id,
                        connection_info,
                        Arc::new((text_channel_id, ctx.http.clone())),
                    )
                    .await?;
                Ok(true)
            }
            Err(e) => {
                error!("Error joining voice channel: {:?}", e);
                Err(BotError::JoinFailure)
            }
        }
    } else {
        Ok(false)
    }
}

pub enum CommandSource<'a> {
    Message(&'a Message),
    Interaction(&'a CommandInteraction),
}

impl<'a> CommandSource<'a> {
    pub async fn reply(&self, ctx: &Context, content: impl Into<String>) -> BotResult<()> {
        let content = content.into();
        match self {
            CommandSource::Message(msg) => {
                msg.reply(ctx, content).await?;
            }
            CommandSource::Interaction(cmd) => {
                if cmd.get_response(&ctx.http).await.is_ok() {
                    cmd.edit_response(&ctx.http, EditInteractionResponse::new().content(content))
                        .await?;
                } else {
                    let response = CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new().content(content),
                    );
                    cmd.create_response(&ctx.http, response).await?;
                }
            }
        }
        Ok(())
    }

    pub async fn reply_with_attachment(
        &self,
        ctx: &Context,
        content: impl Into<String>,
        attachment: CreateAttachment,
    ) -> BotResult<()> {
        let content = content.into();
        match self {
            CommandSource::Message(msg) => {
                msg.channel_id
                    .send_message(
                        &ctx.http,
                        serenity::builder::CreateMessage::new()
                            .content(content)
                            .add_file(attachment),
                    )
                    .await?;
            }
            CommandSource::Interaction(cmd) => {
                if cmd.get_response(&ctx.http).await.is_ok() {
                    cmd.edit_response(
                        &ctx.http,
                        EditInteractionResponse::new()
                            .content(content)
                            .new_attachment(attachment),
                    )
                    .await?;
                } else {
                    let response = CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(content)
                            .add_file(attachment),
                    );
                    cmd.create_response(&ctx.http, response).await?;
                }
            }
        }
        Ok(())
    }

    pub async fn error(&self, ctx: &Context, err: BotError) -> BotResult<()> {
        self.reply(ctx, format!("Error: {}", err.to_message()))
            .await
    }

    pub fn guild_id(&self) -> Option<GuildId> {
        match self {
            CommandSource::Message(msg) => msg.guild_id,
            CommandSource::Interaction(cmd) => cmd.guild_id,
        }
    }

    pub fn user_id(&self) -> UserId {
        match self {
            CommandSource::Message(msg) => msg.author.id,
            CommandSource::Interaction(cmd) => cmd.user.id,
        }
    }

    pub fn channel_id(&self) -> ChannelId {
        match self {
            CommandSource::Message(msg) => msg.channel_id,
            CommandSource::Interaction(cmd) => cmd.channel_id,
        }
    }
}

pub struct CommandContext<'a> {
    pub ctx: &'a Context,
    pub source: CommandSource<'a>,
}

impl<'a> CommandContext<'a> {
    pub fn new(ctx: &'a Context, source: CommandSource<'a>) -> Self {
        Self { ctx, source }
    }

    pub async fn reply(&self, content: impl Into<String>) -> BotResult<()> {
        self.source.reply(self.ctx, content).await
    }

    pub async fn reply_with_attachment(
        &self,
        content: impl Into<String>,
        attachment: CreateAttachment,
    ) -> BotResult<()> {
        self.source
            .reply_with_attachment(self.ctx, content, attachment)
            .await
    }

    pub async fn error(&self, err: BotError) -> BotResult<()> {
        self.source.error(self.ctx, err).await
    }

    pub fn guild_id(&self) -> BotResult<GuildId> {
        self.source.guild_id().ok_or(BotError::MissingGuildId)
    }

    pub fn user_id(&self) -> UserId {
        self.source.user_id()
    }

    pub fn channel_id(&self) -> ChannelId {
        self.source.channel_id()
    }

    pub async fn play_tracks(&self, tracks: Vec<TrackInQueue>) -> BotResult<bool> {
        let guild_id = self.guild_id()?;
        let user_id = self.user_id();
        let channel_id = self.channel_id();

        let lava_client = get_lava_client(self.ctx).await;

        let voice_channel_id = get_user_voice_channel(self.ctx, guild_id, user_id)
            .ok_or(BotError::NotInVoiceChannel)?;

        let has_joined = ensure_joined(
            self.ctx,
            &lava_client,
            guild_id,
            voice_channel_id,
            channel_id,
        )
        .await?;

        let data = self.ctx.data.read().await;
        if let Some(map) = data.get::<InactivityMap>() {
            map.remove(&guild_id);
        }

        let player = lava_client
            .get_player_context(guild_id)
            .ok_or(BotError::NoActivePlayer)?;

        let queue = player.get_queue();
        queue.append(tracks.into())?;

        if let Ok(player_data) = player.get_player().await {
            if player_data.track.is_none() && queue.get_track(0).await.is_ok_and(|x| x.is_some()) {
                let _ = player.skip();
            }
        }

        Ok(has_joined)
    }
}
