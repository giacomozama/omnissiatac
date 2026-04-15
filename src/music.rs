use std::sync::Arc;
use std::time::Instant;

use lavalink_rs::{hook, model::events, prelude::*};
use serenity::builder::{
    CreateCommand, CreateCommandOption, CreateEmbed, CreateEmbedFooter, CreateMessage,
};
use serenity::model::application::{CommandInteraction, CommandOptionType};
use serenity::model::channel::Message;
use serenity::model::id::{ChannelId, UserId};
use serenity::prelude::*;
use tracing::{error, info};

use crate::common::{CommandContext, CommandSource, InactivityMap, get_lava_client};
use crate::error::{BotError, BotResult};

#[hook]
pub async fn track_start(client: LavalinkClient, _session_id: String, event: &events::TrackStart) {
    info!("Track started on guild: {:?}", event.guild_id);

    let player = match client.get_player_context(event.guild_id) {
        Some(p) => p,
        None => return,
    };

    let data = player.data::<(ChannelId, Arc<serenity::http::Http>)>();
    let (channel_id, http) = match data.ok() {
        Some(d) => (d.0, d.1.clone()),
        None => return,
    };

    let track = &event.track;
    let mut embed = CreateEmbed::new()
        .title("Now Playing")
        .description(format!(
            "[{}]({})",
            track.info.title,
            track.info.uri.as_deref().unwrap_or("")
        ))
        .field("Author", &track.info.author, true)
        .field(
            "Length",
            format!(
                "{}:{:02}",
                track.info.length / 60000,
                (track.info.length / 1000) % 60
            ),
            true,
        );

    if let Some(thumbnail) = &track.info.artwork_url {
        embed = embed.thumbnail(thumbnail);
    }

    if let Some(user_data) = &track.user_data {
        if let Some(requester_id) = user_data.get("requester_id").and_then(|id| id.as_u64()) {
            let user_id = UserId::new(requester_id);
            let requester_name = match http.get_member(event.guild_id.0.into(), user_id).await {
                Ok(member) => member.display_name().to_owned(),
                Err(_) => format!("{}", user_id),
            };
            embed = embed.footer(CreateEmbedFooter::new(format!(
                "Requested by: {}",
                requester_name
            )));
        }
    }

    let _ = channel_id
        .send_message(http.as_ref(), CreateMessage::new().embed(embed))
        .await;
}

#[hook]
pub async fn track_end(_client: LavalinkClient, _session_id: String, event: &events::TrackEnd) {
    info!("Track ended on guild: {:?}", event.guild_id);
}

#[hook]
pub async fn track_exception(
    client: LavalinkClient,
    _session_id: String,
    event: &events::TrackException,
) {
    error!(
        "Track exception on guild {:?}: {:?}",
        event.guild_id, event.exception
    );

    let player = match client.get_player_context(event.guild_id) {
        Some(p) => p,
        None => return,
    };

    let data = player.data::<(ChannelId, Arc<serenity::http::Http>)>();
    let (channel_id, http) = match data.ok() {
        Some(d) => (d.0, d.1.clone()),
        None => return,
    };

    let error_msg = format!(
        "❌ **Playback Error**\nSomething went wrong while playing: `{}`\nError: `{}`",
        event.track.info.title, event.exception.message
    );

    let _ = channel_id
        .send_message(http.as_ref(), CreateMessage::new().content(error_msg))
        .await;
}

#[hook]
pub async fn ready_event(client: LavalinkClient, session_id: String, event: &events::Ready) {
    info!(
        "Lavalink node ready! Session ID: {}, Event: {:?}",
        session_id, event
    );
    let _ = client.delete_all_player_contexts().await;
}

pub fn register_play() -> CreateCommand {
    CreateCommand::new("play")
        .description("Play a song from YouTube or a URL")
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "query", "The song to play")
                .required(true),
        )
}

pub fn register_stop() -> CreateCommand {
    CreateCommand::new("stop").description("Stop the music and clear the queue")
}

pub fn register_skip() -> CreateCommand {
    CreateCommand::new("skip").description("Skip the current song")
}

pub fn register_leave() -> CreateCommand {
    CreateCommand::new("leave").description("Leave the voice channel immediately")
}

pub async fn perform_play(ctx: &CommandContext<'_>, query_str: &str) -> BotResult<String> {
    let lava_client = get_lava_client(ctx.ctx).await;
    let guild_id = ctx.guild_id()?;
    let user_id = ctx.user_id();

    info!(
        "Play request from user {} in guild {} with query: {}",
        user_id, guild_id, query_str
    );

    let query = if query_str.starts_with("http") {
        query_str.to_string()
    } else {
        SearchEngines::YouTube
            .to_query(query_str)
            .map_err(|_| BotError::InvalidQuery)?
    };

    let loaded_tracks = lava_client.load_tracks(guild_id, &query).await?;
    let mut tracks: Vec<TrackInQueue> = match loaded_tracks.data {
        Some(TrackLoadData::Track(x)) => {
            info!("Loaded single track: {}", x.info.title);
            vec![x.into()]
        }
        Some(TrackLoadData::Search(x)) => {
            if x.is_empty() {
                return Err(BotError::NoTracksFound);
            }
            info!("Loaded search result: {}", x[0].info.title);
            vec![x[0].clone().into()]
        }
        Some(TrackLoadData::Playlist(x)) => {
            info!(
                "Loaded playlist: {} ({} tracks)",
                x.info.name,
                x.tracks.len()
            );
            x.tracks.iter().map(|x| x.clone().into()).collect()
        }
        _ => return Err(BotError::NoTracksFound),
    };

    if tracks.is_empty() {
        return Err(BotError::NoTracksFound);
    }

    for i in &mut tracks {
        i.track.user_data = Some(serde_json::json!({ "requester_id": user_id.get() }));
    }

    let has_joined = ctx.play_tracks(tracks).await?;

    if has_joined {
        Ok("Joined and added to queue".to_string())
    } else {
        Ok("Added to queue".to_string())
    }
}

pub async fn perform_stop(ctx: &CommandContext<'_>) -> BotResult<String> {
    let lava_client = get_lava_client(ctx.ctx).await;
    let guild_id = ctx.guild_id()?;

    info!("Stop request in guild {}", guild_id);

    let player = lava_client
        .get_player_context(guild_id)
        .ok_or(BotError::NoActivePlayer)?;

    player.stop_now().await?;
    player.get_queue().clear()?;

    let data = ctx.ctx.data.read().await;
    if let Some(map) = data.get::<InactivityMap>() {
        map.insert(guild_id, Instant::now());
    }

    Ok("Stopped and cleared queue".to_string())
}

pub async fn perform_skip(ctx: &CommandContext<'_>) -> BotResult<String> {
    let lava_client = get_lava_client(ctx.ctx).await;
    let guild_id = ctx.guild_id()?;

    info!("Skip request in guild {}", guild_id);

    let player = lava_client
        .get_player_context(guild_id)
        .ok_or(BotError::NoActivePlayer)?;

    player.skip()?;

    let data = ctx.ctx.data.read().await;
    if let Some(map) = data.get::<InactivityMap>() {
        map.remove(&guild_id);
    }

    Ok("Skipped".to_string())
}

pub async fn perform_leave(ctx: &CommandContext<'_>) -> BotResult<String> {
    let guild_id = ctx.guild_id()?;
    let lava_client = get_lava_client(ctx.ctx).await;

    let manager = songbird::get(ctx.ctx)
        .await
        .expect("Songbird manager not found")
        .clone();

    info!("Leave request in guild {}", guild_id);

    // Leave the voice channel
    let _ = manager.leave(guild_id).await;

    // Clean up Lavalink player context
    if let Some(player) = lava_client.get_player_context(guild_id) {
        let _ = player.stop_now().await;
        let _ = player.get_queue().clear();
    }

    // Clean up inactivity map
    let data = ctx.ctx.data.read().await;
    if let Some(map) = data.get::<InactivityMap>() {
        map.remove(&guild_id);
    }

    Ok("Disconnected from voice channel".to_string())
}

pub async fn play(ctx: &Context, msg: &Message, term: &str) {
    let ctx = CommandContext::new(ctx, CommandSource::Message(msg));
    if term.is_empty() {
        let _ = ctx.reply("Please provide a search term or URL").await;
        return;
    }

    match perform_play(&ctx, term).await {
        Ok(res) => {
            let _ = ctx.reply(res).await;
        }
        Err(e) => {
            let _ = ctx.error(e).await;
        }
    }
}

pub async fn stop(ctx: &Context, msg: &Message) {
    let ctx = CommandContext::new(ctx, CommandSource::Message(msg));
    match perform_stop(&ctx).await {
        Ok(res) => {
            let _ = ctx.reply(res).await;
        }
        Err(e) => {
            let _ = ctx.error(e).await;
        }
    }
}

pub async fn skip(ctx: &Context, msg: &Message) {
    let ctx = CommandContext::new(ctx, CommandSource::Message(msg));
    match perform_skip(&ctx).await {
        Ok(res) => {
            let _ = ctx.reply(res).await;
        }
        Err(e) => {
            let _ = ctx.error(e).await;
        }
    }
}

pub async fn leave(ctx: &Context, msg: &Message) {
    let ctx = CommandContext::new(ctx, CommandSource::Message(msg));
    match perform_leave(&ctx).await {
        Ok(res) => {
            let _ = ctx.reply(res).await;
        }
        Err(e) => {
            let _ = ctx.error(e).await;
        }
    }
}

pub async fn slash_play(ctx: &Context, command: &CommandInteraction) {
    let ctx = CommandContext::new(ctx, CommandSource::Interaction(command));
    let term = command
        .data
        .options
        .first()
        .and_then(|opt| opt.value.as_str())
        .unwrap_or("");

    if term.is_empty() {
        let _ = ctx.reply("Please provide a search term or URL").await;
        return;
    }

    let _ = command.defer(&ctx.ctx.http).await;

    match perform_play(&ctx, term).await {
        Ok(res) => {
            let _ = ctx.reply(res).await;
        }
        Err(e) => {
            let _ = ctx.error(e).await;
        }
    }
}

pub async fn slash_stop(ctx: &Context, command: &CommandInteraction) {
    let _ = command.defer(&ctx.http).await;
    let ctx = CommandContext::new(ctx, CommandSource::Interaction(command));
    match perform_stop(&ctx).await {
        Ok(res) => {
            let _ = ctx.reply(res).await;
        }
        Err(e) => {
            let _ = ctx.error(e).await;
        }
    }
}

pub async fn slash_skip(ctx: &Context, command: &CommandInteraction) {
    let _ = command.defer(&ctx.http).await;
    let ctx = CommandContext::new(ctx, CommandSource::Interaction(command));
    match perform_skip(&ctx).await {
        Ok(res) => {
            let _ = ctx.reply(res).await;
        }
        Err(e) => {
            let _ = ctx.error(e).await;
        }
    }
}

pub async fn slash_leave(ctx: &Context, command: &CommandInteraction) {
    let _ = command.defer(&ctx.http).await;
    let ctx = CommandContext::new(ctx, CommandSource::Interaction(command));
    match perform_leave(&ctx).await {
        Ok(res) => {
            let _ = ctx.reply(res).await;
        }
        Err(e) => {
            let _ = ctx.error(e).await;
        }
    }
}
