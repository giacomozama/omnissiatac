use std::fs::{self, OpenOptions};
use std::io::{BufRead, Write};
use std::path::Path;

use lavalink_rs::prelude::*;
use serenity::builder::{CreateCommand, CreateCommandOption};
use serenity::model::application::{CommandInteraction, CommandOptionType};
use serenity::model::id::GuildId;
use serenity::prelude::*;
use tracing::info;

use crate::common::{CommandContext, CommandSource, get_lava_client};
use crate::error::{BotError, BotResult};

pub const PLAYLISTS_DIR: &str = "playlists";

pub fn get_playlist_path(name: &str) -> String {
    format!("{}/{}.txt", PLAYLISTS_DIR, name)
}

pub async fn create_playlist(name: &str) -> BotResult<()> {
    fs::create_dir_all(PLAYLISTS_DIR)?;
    let path = get_playlist_path(name);
    if Path::new(&path).exists() {
        return Err(BotError::PlaylistAlreadyExists);
    }
    info!("Creating new playlist: {}", name);
    fs::File::create(path)?;
    Ok(())
}

pub async fn add_to_playlist(
    lava_client: &LavalinkClient,
    guild_id: GuildId,
    playlist_name: &str,
    query_str: &str,
) -> BotResult<usize> {
    let path = get_playlist_path(playlist_name);
    if !Path::new(&path).exists() {
        return Err(BotError::PlaylistNotFound);
    }

    info!(
        "Adding to playlist '{}' in guild {}: {}",
        playlist_name, guild_id, query_str
    );

    let query = if query_str.starts_with("http") {
        query_str.to_string()
    } else {
        SearchEngines::YouTube
            .to_query(query_str)
            .map_err(|_| BotError::InvalidQuery)?
    };

    let loaded_tracks = lava_client.load_tracks(guild_id, &query).await?;

    let uris: Vec<String> = match loaded_tracks.data {
        Some(TrackLoadData::Track(x)) => x.info.uri.into_iter().collect(),
        Some(TrackLoadData::Search(x)) => x
            .first()
            .and_then(|t| t.info.uri.clone())
            .into_iter()
            .collect(),
        Some(TrackLoadData::Playlist(x)) => {
            x.tracks.iter().filter_map(|t| t.info.uri.clone()).collect()
        }
        _ => return Err(BotError::NoTracksFound),
    };

    if uris.is_empty() {
        return Err(BotError::NoTracksFound);
    }

    info!(
        "Found {} tracks to add to playlist '{}'",
        uris.len(),
        playlist_name
    );

    let mut file = OpenOptions::new().append(true).open(&path)?;

    for uri in &uris {
        writeln!(file, "{}", uri)?;
    }

    Ok(uris.len())
}

pub fn load_playlist(name: &str) -> BotResult<Vec<String>> {
    let path = get_playlist_path(name);
    if !Path::new(&path).exists() {
        return Err(BotError::PlaylistNotFound);
    }
    let file = fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut uris = Vec::new();
    for line in reader.lines() {
        if let Ok(uri) = line {
            if !uri.trim().is_empty() {
                uris.push(uri.trim().to_string());
            }
        }
    }
    Ok(uris)
}

pub fn list_playlists() -> BotResult<Vec<String>> {
    fs::create_dir_all(PLAYLISTS_DIR)?;
    let mut playlists = Vec::new();
    for entry in fs::read_dir(PLAYLISTS_DIR)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("txt") {
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                playlists.push(name.to_string());
            }
        }
    }
    Ok(playlists)
}

pub fn register_playlist() -> CreateCommand {
    CreateCommand::new("playlist")
        .description("Manage playlists")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "create",
                "Create a new playlist",
            )
            .add_sub_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "name",
                    "The name of the playlist",
                )
                .required(true),
            ),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                "add",
                "Add a song to a playlist",
            )
            .add_sub_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "name",
                    "The name of the playlist",
                )
                .required(true),
            )
            .add_sub_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "query",
                    "The song or playlist URL to add",
                )
                .required(true),
            ),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::SubCommand, "play", "Play a playlist")
                .add_sub_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "name",
                        "The name of the playlist",
                    )
                    .required(true),
                ),
        )
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "list",
            "List all playlists",
        ))
}

pub async fn perform_play_playlist(
    ctx: &CommandContext<'_>,
    uris: Vec<String>,
) -> BotResult<String> {
    let lava_client = get_lava_client(ctx.ctx).await;
    let guild_id = ctx.guild_id()?;
    let user_id = ctx.user_id();

    info!(
        "Playing playlist for user {} in guild {} ({} entries)",
        user_id,
        guild_id,
        uris.len()
    );

    let mut total_added = 0;
    for uri in uris {
        if let Ok(loaded_tracks) = lava_client.load_tracks(guild_id, &uri).await {
            let tracks: Vec<TrackInQueue> = match loaded_tracks.data {
                Some(TrackLoadData::Track(x)) => vec![x.into()],
                Some(TrackLoadData::Search(x)) => {
                    if x.is_empty() {
                        continue;
                    }
                    vec![x[0].clone().into()]
                }
                Some(TrackLoadData::Playlist(x)) => {
                    x.tracks.iter().map(|x| x.clone().into()).collect()
                }
                _ => continue,
            };

            let mut tracks_to_queue = Vec::new();
            for mut track in tracks {
                track.track.user_data = Some(serde_json::json!({ "requester_id": user_id.get() }));
                tracks_to_queue.push(track);
                total_added += 1;
            }
            let _ = ctx.play_tracks(tracks_to_queue).await;
        }
    }

    info!(
        "Finished queuing playlist, added {} total tracks",
        total_added
    );

    Ok(format!(
        "Added {} tracks from playlist to queue",
        total_added
    ))
}

pub async fn slash_playlist(ctx_orig: &Context, command: &CommandInteraction) {
    let _ = command.defer(&ctx_orig.http).await;
    let ctx = CommandContext::new(ctx_orig, CommandSource::Interaction(command));
    let options = &command.data.options;
    let subcommand = match options.first() {
        Some(opt) => opt,
        None => return,
    };

    let sub_options = match &subcommand.value {
        serenity::model::application::CommandDataOptionValue::SubCommand(opts) => opts,
        _ => return,
    };

    match subcommand.name.as_str() {
        "create" => {
            let name = sub_options
                .first()
                .and_then(|o| o.value.as_str())
                .unwrap_or("");
            match create_playlist(name).await {
                Ok(_) => {
                    let _ = ctx.reply(format!("Playlist '{}' created", name)).await;
                }
                Err(e) => {
                    let _ = ctx.error(e).await;
                }
            }
        }
        "add" => {
            let name = sub_options
                .iter()
                .find(|o| o.name == "name")
                .and_then(|o| o.value.as_str())
                .unwrap_or("");
            let query = sub_options
                .iter()
                .find(|o| o.name == "query")
                .and_then(|o| o.value.as_str())
                .unwrap_or("");

            let guild_id = match ctx.guild_id() {
                Ok(id) => id,
                Err(e) => {
                    let _ = ctx.error(e).await;
                    return;
                }
            };
            let lava_client = get_lava_client(ctx.ctx).await;
            match add_to_playlist(&lava_client, guild_id, name, query).await {
                Ok(count) => {
                    let _ = ctx
                        .reply(format!("Added {} track(s) to playlist '{}'", count, name))
                        .await;
                }
                Err(e) => {
                    let _ = ctx.error(e).await;
                }
            }
        }
        "play" => {
            let name = sub_options
                .first()
                .and_then(|o| o.value.as_str())
                .unwrap_or("");
            match load_playlist(name) {
                Ok(uris) => match perform_play_playlist(&ctx, uris).await {
                    Ok(res) => {
                        let _ = ctx.reply(res).await;
                    }
                    Err(e) => {
                        let _ = ctx.error(e).await;
                    }
                },
                Err(e) => {
                    let _ = ctx.error(e).await;
                }
            }
        }
        "list" => match list_playlists() {
            Ok(playlists) => {
                let content = if playlists.is_empty() {
                    "No playlists found".to_string()
                } else {
                    format!("Available playlists:\n{}", playlists.join("\n"))
                };
                let _ = ctx.reply(content).await;
            }
            Err(e) => {
                let _ = ctx.error(e).await;
            }
        },
        _ => (),
    }
}
