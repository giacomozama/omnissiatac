use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use lavalink_rs::{model::events, prelude::*};
use rand::distr::{Alphanumeric, SampleString};
use serenity::async_trait;
use serenity::builder::{CreateCommand, CreateCommandOption};
use serenity::model::application::CommandOptionType;
use serenity::http::Http;
use serenity::model::application::{Command, CommandInteraction, Interaction};
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::model::id::{ChannelId, GuildId};
use serenity::prelude::*;
use songbird::serenity::SerenityInit;
use tracing::{error, info};

mod common;
mod config;
mod error;
mod llm;
mod music;
mod playlist;
mod slop;
mod tts;
mod web;

use crate::common::{
    get_lava_client, ChatHistory, CommandContext, CommandSource, ConfigKey, InactivityMap, Lavalink,
};
use crate::config::Config;
use crate::playlist::perform_play_playlist;

const HELP_TEXT: &str = "
`╔═════════════════════════════════════════════════╗`
`║ ▗▄▖ ▄▄▄▄  ▄▄▄▄  ▄  ▄▄▄  ▄▄▄ ▄  ▗▄▖▗▄▄▄▖▗▄▖  ▗▄▄▖║`
`║▐▌ ▐▌█ █ █ █   █ ▄ ▀▄▄  ▀▄▄  ▄ ▐▌ ▐▌ █ ▐▌ ▐▌▐▌   ║`
`║▐▌ ▐▌█   █ █   █ █ ▄▄▄▀ ▄▄▄▀ █ ▐▛▀▜▌ █ ▐▛▀▜▌▐▌   ║`
`║▝▚▄▞▘            █           █ ▐▌ ▐▌ █ ▐▌ ▐▌▝▚▄▄▖║`
`╚═════════════════════════════════════════════════╝`

- `play <query>` (p): Play a song from YouTube or a URL.
- `say <text>` (s): Convert text to speech (guesses language).
- `sayin <lang> <text>` (si): Convert text to speech in a specific language.
- `slop <prompt>` (i): Generate AI art from the Machine Spirit's forge.
- `skip` (sk): Skip the current song.
- `stop` (st): Stop the music and clear the queue.
- `leave` (l, kys): Leave the voice channel immediately.
- `playlist create <name>` (pl c): Create a new playlist.
- `playlist add <name> <query>` (pl a): Add a song or playlist to a bot playlist.
- `playlist play <name>` (pl p): Play a bot playlist.
- `playlist list` (pl ls): List all bot playlists.
- `llm reset` (llm r): Reset conversation history for this channel.
- `help` (h): Show this help message.

**Conversational Mode:**
- Mention me (@OmnissiATAC) to chat! I'm powered by a local Machine Spirit (LLM).";

struct Handler;

async fn help(ctx: &Context, msg: &Message) {
    let ctx = CommandContext::new(ctx, CommandSource::Message(msg));
    let text = format!(
        "{}

*Note: You can also use Slash Commands (/) for all of these!*",
        HELP_TEXT
    );
    let _ = ctx.reply(text).await;
}

async fn slash_help(ctx: &Context, command: &CommandInteraction) {
    let ctx = CommandContext::new(ctx, CommandSource::Interaction(command));
    let _ = ctx.reply(HELP_TEXT).await;
}

async fn slash_llm(ctx: &Context, command: &CommandInteraction) {
    let _ = command.defer(&ctx.http).await;
    let ctx_cmd = CommandContext::new(ctx, CommandSource::Interaction(command));
    let option = command.data.options.iter().find(|o| o.name == "reset");
    
    if option.is_some() {
        info!("Resetting LLM history for channel {} (Slash Command)", command.channel_id);
        let data = ctx.data.read().await;
        let history = data.get::<ChatHistory>().expect("ChatHistory not found");
        history.remove(&command.channel_id);
        let _ = ctx_cmd.reply("Conversation history has been purged. The Machine Spirit is refreshed.").await;
    } else {
        let _ = ctx_cmd.reply("Unknown LLM command. Use `/llm reset`.").await;
    }
}

async fn handle_playlist_prefix(ctx: &Context, msg: &Message, content: &str) {
    let ctx = CommandContext::new(ctx, CommandSource::Message(msg));
    let parts: Vec<&str> = content.split_whitespace().collect();
    if parts.is_empty() {
        let _ = ctx
            .reply("Usage: `!playlist <create|add|play|list> [args]`")
            .await;
        return;
    }

    match parts[0] {
        "create" | "c" => {
            if parts.len() < 2 {
                let _ = ctx.reply("Usage: `!playlist create <name>`").await;
                return;
            }
            let name = parts[1];
            match playlist::create_playlist(name).await {
                Ok(_) => {
                    let _ = ctx.reply(format!("Playlist '{}' created", name)).await;
                }
                Err(e) => {
                    let _ = ctx.error(e).await;
                }
            }
        }
        "add" | "a" => {
            if parts.len() < 3 {
                let _ = ctx.reply("Usage: `!playlist add <name> <query>`").await;
                return;
            }
            let name = parts[1];
            let query = parts[2..].join(" ");

            let guild_id = match ctx.guild_id() {
                Ok(id) => id,
                Err(e) => {
                    let _ = ctx.error(e).await;
                    return;
                }
            };
            let lava_client = get_lava_client(ctx.ctx).await;
            match playlist::add_to_playlist(&lava_client, guild_id, name, &query).await {
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
        "play" | "p" => {
            if parts.len() < 2 {
                let _ = ctx.reply("Usage: `!playlist play <name>`").await;
                return;
            }
            let name = parts[1];
            match playlist::load_playlist(name) {
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
        "list" | "ls" => match playlist::list_playlists() {
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
        _ => {
            let _ = ctx
                .reply("Unknown playlist command. Use `!playlist <create|add|play|list>`")
                .await;
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected and ready!", ready.user.name);

        let commands = vec![
            music::register_play(),
            tts::register_say(),
            tts::register_sayin(),
            slop::register_slop(),
            music::register_stop(),
            music::register_skip(),
            music::register_leave(),
            playlist::register_playlist(),
            CreateCommand::new("llm")
                .description("LLM related commands")
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "reset",
                        "Reset conversation history for this channel",
                    )
                ),
            CreateCommand::new("help").description("Show help message"),
        ];

        info!("Registering {} global slash commands...", commands.len());
        for command in commands {
            if let Err(e) = Command::create_global_command(&ctx.http, command).await {
                error!("Failed to register command: {:?}", e);
            }
        }

        info!("Successfully registered global slash commands.");
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = interaction {
            info!("Received slash command: {} from user {}", command.data.name, command.user.name);
            match command.data.name.as_str() {
                "play" => music::slash_play(&ctx, &command).await,
                "say" => tts::slash_say(&ctx, &command).await,
                "sayin" => tts::slash_sayin(&ctx, &command).await,
                "slop" => slop::slash_slop(&ctx, &command).await,
                "stop" => music::slash_stop(&ctx, &command).await,
                "skip" => music::slash_skip(&ctx, &command).await,
                "leave" => music::slash_leave(&ctx, &command).await,
                "playlist" => playlist::slash_playlist(&ctx, &command).await,
                "llm" => slash_llm(&ctx, &command).await,
                "help" => slash_help(&ctx, &command).await,
                _ => (),
            };
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        if msg.content.starts_with("!") {
            info!("Received prefix command: {} from user {}", msg.content, msg.author.name);
            
            let parts: Vec<&str> = msg.content[1..].splitn(2, ' ').collect();
            let cmd = parts[0];
            let content = if parts.len() > 1 { parts[1] } else { "" };

            match cmd {
                "play" | "p" => music::play(&ctx, &msg, content).await,
                "say" | "s" => tts::say(&ctx, &msg, content).await,
                "sayin" | "si" => tts::sayin(&ctx, &msg, content).await,
                "slop" | "i" => slop::slop(&ctx, &msg, content).await,
                "playlist" | "pl" => handle_playlist_prefix(&ctx, &msg, content).await,
                "stop" | "st" => music::stop(&ctx, &msg).await,
                "skip" | "sk" => music::skip(&ctx, &msg).await,
                "leave" | "l" | "kys" => music::leave(&ctx, &msg).await,
                "llm" => {
                    if content == "reset" || content == "r" {
                        let data = ctx.data.read().await;
                        let history = data.get::<ChatHistory>().expect("ChatHistory not found");
                        history.remove(&msg.channel_id);
                        let _ = msg.reply(&ctx.http, "Conversation history has been purged. The Machine Spirit is refreshed.").await;
                    } else {
                        let _ = msg.reply(&ctx.http, "Unknown LLM command. Use `!llm reset`.").await;
                    }
                }
                "help" | "h" => help(&ctx, &msg).await,
                _ => (),
            }
            return;
        }

        // Conversational LLM handle
        if msg.mentions_me(&ctx.http).await.unwrap_or(false) {
            let data = ctx.data.read().await;
            let config_lock = data.get::<ConfigKey>().expect("ConfigKey not found").clone();
            let config = config_lock.read().await;
            
            let ollama_config = config.ollama.clone();
            let history_map = data.get::<ChatHistory>().expect("ChatHistory not found").clone();
            
            // Remove the bot mention from the content to get the actual prompt
            let bot_id = match ctx.http.get_current_user().await {
                Ok(u) => u.id,
                Err(_) => return, // Should not happen usually
            };
            let mention = format!("<@{}>", bot_id);
            let mention_nick = format!("<@!{}>", bot_id);
            let prompt = msg.content.replace(&mention, "").replace(&mention_nick, "").trim().to_string();
            
            if prompt.is_empty() {
                let _ = msg.reply(&ctx.http, "Praise the Omnissiah! How may I assist you?").await;
                return;
            }

            let ctx_clone = ctx.clone();
            let msg_clone = msg.clone();
            let channel_id = msg.channel_id;

            tokio::spawn(async move {
                let _ = msg_clone.channel_id.broadcast_typing(&ctx_clone.http).await;
                
                let current_history = history_map.get(&channel_id).map(|h| h.clone());
                
                match llm::query_llm(&ollama_config, &prompt, current_history).await {
                    Ok((response, new_history)) => {
                        history_map.insert(channel_id, new_history);
                        if let Err(e) = msg_clone.reply(&ctx_clone.http, response).await {
                            error!("Error sending LLM response: {:?}", e);
                        }
                    }
                    Err(e) => {
                        error!("Error querying LLM: {:?}", e);
                        let _ = msg_clone.reply(&ctx_clone.http, "The Machine Spirit is silent... (Failed to connect to LLM node)").await;
                    }
                }
            });
        }
    }
}

#[tokio::main]
async fn main() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Print ASCII art to terminal
    let ascii_art = HELP_TEXT
        .lines()
        .filter(|line| line.starts_with('`'))
        .map(|line| line.trim_matches('`'))
        .collect::<Vec<_>>()
        .join("\n");
    println!("{}\n\n", ascii_art);

    info!("Starting OmnissiATAC...");
    info!("Loading configuration...");
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            return;
        }
    };

    let shared_config = Arc::new(tokio::sync::RwLock::new(config.clone()));

    // Generate a random JWT secret at runtime
    let jwt_secret = Alphanumeric.sample_string(&mut rand::rng(), 32);

    // Start web server in background
    let web_state = web::AppState {
        config: shared_config.clone(),
        jwt_secret,
    };
    tokio::spawn(async move {
        web::start_web_server(web_state).await;
    });

    info!("Loading environment variables...");
    // Fallback to environment variables if needed, but prefer config
    let token = std::env::var("DISCORD_TOKEN").unwrap_or_else(|_| config.discord.token.clone());
    
    // Update config with env var overrides for components that are used via config in TypeMap
    let mut config = config;
    if let Ok(url) = std::env::var("OLLAMA_BASE_URL") {
        config.ollama.base_url = url;
    }
    if let Ok(url) = std::env::var("COMFY_BASE_URL") {
        config.comfy.base_url = url;
    }

    let host = std::env::var("LAVALINK_HOST").unwrap_or_else(|_| config.lavalink.host.clone());
    let port = std::env::var("LAVALINK_PORT")
        .map(|p| p.parse().unwrap_or(config.lavalink.port))
        .unwrap_or(config.lavalink.port);
    let password = std::env::var("LAVALINK_PASSWORD").unwrap_or_else(|_| config.lavalink.password.clone());
    let is_ssl = config.lavalink.is_ssl;

    info!("Configuring gateway intents...");
    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::GUILD_VOICE_STATES;

    info!("Initializing HTTP client to fetch bot info...");
    let http = Http::new(&token);
    let bot_id = match http.get_current_user().await {
        Ok(user) => user.id,
        Err(e) => {
            error!("Failed to fetch bot user info: {:?}", e);
            return;
        }
    };
    info!("Bot ID: {}, Username: {}", bot_id, http.get_current_user().await.map(|u| u.name.clone()).unwrap_or_else(|_| "Unknown".to_string()));

    info!("Initializing Lavalink client...");
    let lava_events = events::Events {
        ready: Some(music::ready_event),
        track_start: Some(music::track_start),
        track_end: Some(music::track_end),
        track_exception: Some(music::track_exception),
        ..Default::default()
    };

    let node_local = NodeBuilder {
        hostname: format!("{}:{}", host, port),
        is_ssl: is_ssl,
        events: events::Events::default(),
        password: password,
        user_id: bot_id.into(),
        session_id: None,
    };

    let lava_client = LavalinkClient::new(
        lava_events,
        vec![node_local],
        NodeDistributionStrategy::round_robin(),
    )
    .await;

    info!("Attempting to connect to Lavalink node at {}...", host);
    loop {
        let mut connected = false;
        for node in &lava_client.nodes {
            if node.connect(lava_client.clone()).await.is_ok() {
                connected = true;
                break;
            }
        }

        if connected {
            info!("Successfully connected to Lavalink!");
            break;
        }

        error!("Failed to connect to Lavalink node {}, retrying in 5 seconds...", host);
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    info!("Initializing inactivity monitor...");
    let inactivity_map = Arc::new(DashMap::<GuildId, Instant>::new());
    let chat_history = Arc::new(DashMap::<ChannelId, Vec<llm::ChatMessage>>::new());
    let songbird_manager = songbird::Songbird::serenity();

    info!("Building Serenity client...");
    let client_result = Client::builder(&token, intents)
        .event_handler(Handler)
        .register_songbird_with(songbird_manager.clone())
        .type_map_insert::<Lavalink>(lava_client.clone())
        .type_map_insert::<InactivityMap>(inactivity_map.clone())
        .type_map_insert::<ConfigKey>(shared_config.clone())
        .type_map_insert::<ChatHistory>(chat_history.clone())
        .await;

    let mut client = match client_result {
        Ok(c) => c,
        Err(e) => {
            error!("Error creating Serenity client: {:?}", e);
            return;
        }
    };

    info!("Spawning inactivity check background task...");
    let lava_client_clone = lava_client.clone();
    let inactivity_map_clone = inactivity_map.clone();
    let cache_clone = client.cache.clone();
    let inactivity_config = shared_config.clone();

    tokio::spawn(async move {
        info!("Inactivity check task started.");
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;

            let timeout_secs = {
                let cfg = inactivity_config.read().await;
                cfg.bot.inactivity_timeout_seconds
            };
            
            let mut to_leave = Vec::new();

            for guild_id in cache_clone.guilds() {
                if let Some(player) = lava_client_clone.get_player_context(guild_id) {
                    let is_playing = if let Ok(player_data) = player.get_player().await {
                        player_data.track.is_some()
                    } else {
                        false
                    };

                    let queue_empty = player
                        .get_queue()
                        .get_track(0)
                        .await
                        .ok()
                        .flatten()
                        .is_none();

                    if !is_playing && queue_empty {
                        let entry = inactivity_map_clone
                            .entry(guild_id)
                            .or_insert_with(Instant::now);
                        if entry.elapsed() >= Duration::from_secs(timeout_secs) {
                            to_leave.push(guild_id);
                        }
                    } else {
                        inactivity_map_clone.remove(&guild_id);
                    }
                }
            }

            for guild_id in to_leave {
                info!("Leaving guild {:?} due to inactivity", guild_id);
                let _ = songbird_manager.leave(guild_id).await;
                if let Some(player) = lava_client_clone.get_player_context(guild_id) {
                    let _ = player.stop_now().await;
                    let _ = player.get_queue().clear();
                }
                inactivity_map_clone.remove(&guild_id);
            }
        }
    });

    if let Err(why) = client.start().await {
        error!("Serenity client error: {:?}", why);
    }
}
