use lavalink_rs::prelude::*;
use serenity::builder::{CreateCommand, CreateCommandOption};
use serenity::model::application::{CommandInteraction, CommandOptionType};
use serenity::model::channel::Message;
use serenity::prelude::*;
use tracing::info;
use whatlang::{Lang, detect};

use crate::common::{CommandContext, CommandSource, get_lava_client};
use crate::error::{BotError, BotResult};

fn lang_to_iso639_1(lang: Lang) -> &'static str {
    match lang {
        Lang::Afr => "af",
        Lang::Aka => "ak",
        Lang::Amh => "am",
        Lang::Ara => "ar",
        Lang::Aze => "az",
        Lang::Bel => "be",
        Lang::Ben => "bn",
        Lang::Bul => "bg",
        Lang::Cat => "ca",
        Lang::Ces => "cs",
        Lang::Cmn => "zh",
        Lang::Cym => "cy",
        Lang::Dan => "da",
        Lang::Deu => "de",
        Lang::Ell => "el",
        Lang::Eng => "en",
        Lang::Epo => "eo",
        Lang::Est => "et",
        Lang::Fin => "fi",
        Lang::Fra => "fr",
        Lang::Guj => "gu",
        Lang::Heb => "he",
        Lang::Hin => "hi",
        Lang::Hrv => "hr",
        Lang::Hun => "hu",
        Lang::Hye => "hy",
        Lang::Ind => "id",
        Lang::Ita => "it",
        Lang::Jav => "jw",
        Lang::Jpn => "ja",
        Lang::Kan => "kn",
        Lang::Kat => "ka",
        Lang::Khm => "km",
        Lang::Kor => "ko",
        Lang::Lav => "lv",
        Lang::Lit => "lt",
        Lang::Mal => "ml",
        Lang::Mar => "mr",
        Lang::Mkd => "mk",
        Lang::Mya => "my",
        Lang::Nep => "ne",
        Lang::Nld => "nl",
        Lang::Nob => "no",
        Lang::Ori => "or",
        Lang::Pan => "pa",
        Lang::Pes => "fa",
        Lang::Pol => "pl",
        Lang::Por => "pt",
        Lang::Ron => "ro",
        Lang::Rus => "ru",
        Lang::Sin => "si",
        Lang::Slk => "sk",
        Lang::Slv => "sl",
        Lang::Sna => "sn",
        Lang::Srp => "sr",
        Lang::Swe => "sv",
        Lang::Tam => "ta",
        Lang::Tel => "te",
        Lang::Tgl => "tl",
        Lang::Tha => "th",
        Lang::Tuk => "tk",
        Lang::Tur => "tr",
        Lang::Ukr => "uk",
        Lang::Urd => "ur",
        Lang::Uzb => "uz",
        Lang::Vie => "vi",
        Lang::Yid => "yi",
        Lang::Zul => "zu",
        Lang::Lat => "la",
        _ => "en",
    }
}

pub fn register_say() -> CreateCommand {
    CreateCommand::new("say")
        .description("Convert text to speech (guesses language)")
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "text", "The text to speak")
                .required(true),
        )
}

pub fn register_sayin() -> CreateCommand {
    CreateCommand::new("sayin")
        .description("Convert text to speech in a specific language")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "lang",
                "The language code (e.g. en, it, fr)",
            )
            .required(true),
        )
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "text", "The text to speak")
                .required(true),
        )
}

async fn perform_say_with_lang(
    ctx: &CommandContext<'_>,
    text: &str,
    lang: &str,
) -> BotResult<String> {
    let lava_client = get_lava_client(ctx.ctx).await;
    let guild_id = ctx.guild_id()?;
    let user_id = ctx.user_id();

    info!(
        "TTS request from user {} in guild {} (lang: {}): {}",
        user_id, guild_id, lang, text
    );

    let encoded_text = urlencoding::encode(text);
    let url = format!(
        "https://translate.google.com/translate_tts?ie=UTF-8&client=tw-ob&tl={}&q={}",
        lang, encoded_text
    );

    let loaded_tracks = lava_client.load_tracks(guild_id, &url).await?;
    let mut tracks: Vec<TrackInQueue> = match loaded_tracks.data {
        Some(TrackLoadData::Track(x)) => vec![x.into()],
        _ => return Err(BotError::NoTracksFound),
    };

    if tracks.is_empty() {
        return Err(BotError::NoTracksFound);
    }

    for i in &mut tracks {
        i.track.user_data = Some(serde_json::json!({ "requester_id": user_id.get() }));
    }

    ctx.play_tracks(tracks).await?;

    Ok(format!("Playing TTS (lang: {})", lang))
}

pub async fn perform_say(ctx: &CommandContext<'_>, text: &str) -> BotResult<String> {
    let info = detect(text);
    let lang = if let Some(info) = info {
        if info.confidence() > 0.1 {
            lang_to_iso639_1(info.lang())
        } else {
            "en"
        }
    } else {
        "en"
    };
    perform_say_with_lang(ctx, text, lang).await
}

pub async fn perform_sayin(ctx: &CommandContext<'_>, lang: &str, text: &str) -> BotResult<String> {
    perform_say_with_lang(ctx, text, lang).await
}

pub async fn say(ctx: &Context, msg: &Message, text: &str) {
    let ctx = CommandContext::new(ctx, CommandSource::Message(msg));
    if text.is_empty() {
        let _ = ctx.reply("Usage: `!say <text>`").await;
        return;
    }

    match perform_say(&ctx, text).await {
        Ok(res) => {
            let _ = ctx.reply(res).await;
        }
        Err(e) => {
            let _ = ctx.error(e).await;
        }
    }
}

pub async fn sayin(ctx: &Context, msg: &Message, content: &str) {
    let ctx = CommandContext::new(ctx, CommandSource::Message(msg));
    let parts: Vec<&str> = content.splitn(2, ' ').collect();
    if parts.len() < 2 {
        let _ = ctx.reply("Usage: `!sayin <lang> <text>`").await;
        return;
    }

    let lang = parts[0];
    let text = parts[1];

    match perform_sayin(&ctx, lang, text).await {
        Ok(res) => {
            let _ = ctx.reply(res).await;
        }
        Err(e) => {
            let _ = ctx.error(e).await;
        }
    }
}

pub async fn slash_say(ctx: &Context, command: &CommandInteraction) {
    let ctx = CommandContext::new(ctx, CommandSource::Interaction(command));
    let text = command
        .data
        .options
        .first()
        .and_then(|opt| opt.value.as_str())
        .unwrap_or("");

    if text.is_empty() {
        let _ = ctx.reply("Please provide text to speak").await;
        return;
    }

    let _ = command.defer(&ctx.ctx.http).await;

    match perform_say(&ctx, text).await {
        Ok(res) => {
            let _ = ctx.reply(res).await;
        }
        Err(e) => {
            let _ = ctx.error(e).await;
        }
    }
}

pub async fn slash_sayin(ctx: &Context, command: &CommandInteraction) {
    let ctx = CommandContext::new(ctx, CommandSource::Interaction(command));
    let mut lang = "";
    let mut text = "";

    for option in &command.data.options {
        match option.name.as_str() {
            "lang" => lang = option.value.as_str().unwrap_or(""),
            "text" => text = option.value.as_str().unwrap_or(""),
            _ => (),
        }
    }

    if lang.is_empty() || text.is_empty() {
        let _ = ctx.reply("Please provide both language and text").await;
        return;
    }

    let _ = command.defer(&ctx.ctx.http).await;

    match perform_sayin(&ctx, lang, text).await {
        Ok(res) => {
            let _ = ctx.reply(res).await;
        }
        Err(e) => {
            let _ = ctx.error(e).await;
        }
    }
}
