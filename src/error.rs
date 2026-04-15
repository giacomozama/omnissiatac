use lavalink_rs::error::LavalinkError;
use serenity::prelude::SerenityError;
use std::io;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BotError {
    #[error("Discord error: {0}")]
    Serenity(#[from] SerenityError),

    #[error("Lavalink error: {0}")]
    Lavalink(#[from] LavalinkError),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("JSON error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    #[error("Not in a voice channel")]
    NotInVoiceChannel,

    #[error("No active player found for this guild")]
    NoActivePlayer,

    #[error("Failed to join voice channel")]
    JoinFailure,

    #[error("No tracks found")]
    NoTracksFound,

    #[error("Playlist already exists")]
    PlaylistAlreadyExists,

    #[error("Playlist does not exist")]
    PlaylistNotFound,

    #[error("Missing guild ID (this command must be used in a server)")]
    MissingGuildId,

    #[error("Invalid query")]
    InvalidQuery,
}

impl BotError {
    pub fn to_message(&self) -> String {
        match self {
            BotError::Serenity(_) => "A Discord error occurred.".to_string(),
            BotError::Lavalink(_) => "A music player error occurred.".to_string(),
            BotError::Io(_) => "A file system error occurred.".to_string(),
            _ => self.to_string(),
        }
    }
}

pub type BotResult<T> = Result<T, BotError>;
