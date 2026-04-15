use crate::common::{CommandContext, CommandSource, ConfigKey};
use crate::config::OllamaConfig;
use crate::llm;
use anyhow::Result;
use feed_rs::parser;
use serenity::builder::CreateCommand;
use serenity::model::application::CommandInteraction;
use serenity::model::channel::Message;
use serenity::prelude::*;
use tracing::error;

fn strip_html(html: &str) -> String {
    // Basic HTML tag removal using regex or a simple state machine.
    // Since we enabled "sanitize" in feed-rs, we might get cleaner HTML, 
    // but we still want just the text for the LLM.
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    // Replace multiple spaces/newlines with single ones
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn register_news() -> CreateCommand {
    CreateCommand::new("news").description("Get the latest news with Machine Spirit commentary")
}

pub async fn slash_news(ctx: &Context, command: &CommandInteraction) {
    let ctx_cmd = CommandContext::new(ctx, CommandSource::Interaction(command));
    handle_news(ctx_cmd).await;
}

pub async fn news(ctx: &Context, msg: &Message) {
    let ctx_cmd = CommandContext::new(ctx, CommandSource::Message(msg));
    handle_news(ctx_cmd).await;
}

async fn handle_news(ctx: CommandContext<'_>) {
    let data = ctx.ctx.data.read().await;
    let config_lock = data
        .get::<ConfigKey>()
        .expect("ConfigKey not found")
        .clone();
    let config = config_lock.read().await;
    let mut ollama_config = config.ollama.clone();
    let bot_name = ctx.ctx.cache.current_user().name.clone();
    if let Some(ref mut system_prompt) = ollama_config.system_prompt {
        *system_prompt = system_prompt.replace("[BOT_NAME]", &bot_name);
    }
    let news_config = config.news.clone();
    drop(data);

    let _ = ctx
        .reply("Consulting the datastreams for the latest occurrences...")
        .await;

    match fetch_and_comment_on_news(&ollama_config, &news_config.feeds).await {
        Ok(report) => {
            if report.titles.is_empty() {
                let _ = ctx.reply("The datastreams are empty. Is the world ending, or is it just a slow news day?").await;
                return;
            }

            let mut full_response = "### 📡 Latest Intelligence from the Datastreams:\n\n".to_string();
            for title in &report.titles {
                full_response.push_str(&format!("- **{}**\n", title));
            }
            full_response.push_str("\n---\n\n");
            full_response.push_str(&report.commentary);
            
            // Discord has a 2000 character limit per message.
            // If the response is too long, we might need to split it.
            if full_response.len() > 1900 {
                let chunks = split_message(&full_response, 1900);
                for chunk in chunks {
                    let _ = ctx.reply(chunk).await;
                }
            } else {
                let _ = ctx.reply(full_response).await;
            }
        }
        Err(e) => {
            error!("Error fetching news: {:?}", e);
            let _ = ctx.reply(format!("Failed to retrieve the datastreams: {}", e)).await;
        }
    }
}

struct NewsReport {
    titles: Vec<String>,
    commentary: String,
}

async fn fetch_and_comment_on_news(
    ollama_config: &OllamaConfig,
    feeds: &[String],
) -> Result<NewsReport> {
    let client = reqwest::Client::builder()
        .user_agent("OmnissiATAC/0.1.0 (Discord Bot)")
        .build()?;
    let mut all_items = Vec::new();

    for feed_url in feeds {
        match client.get(feed_url).send().await {
            Ok(response) => {
                let status = response.status();
                match response.bytes().await {
                    Ok(bytes) => {
                        match parser::parse(&bytes[..]) {
                            Ok(feed) => {
                                for entry in feed.entries {
                                    all_items.push(entry);
                                }
                            }
                            Err(e) => {
                                let body_preview = String::from_utf8_lossy(&bytes[..bytes.len().min(100)]);
                                error!("Failed to parse feed {} (Status: {}). Error: {:?}. Body preview: {}", feed_url, status, e, body_preview);
                            }
                        }
                    }
                    Err(e) => error!("Failed to get bytes from feed {} (Status: {}): {:?}", feed_url, status, e),
                }
            }
            Err(e) => error!("Failed to fetch feed {}: {:?}", feed_url, e),
        }
    }

    // Sort by publication/update date descending
    all_items.sort_by(|a, b| {
        let date_a = a.published.or(a.updated);
        let date_b = b.published.or(b.updated);
        date_b.cmp(&date_a)
    });

    // Take the 10 most recent
    let recent_items = all_items.into_iter().take(10).collect::<Vec<_>>();

    let mut titles = Vec::new();
    let mut articles_text = String::new();

    for (i, item) in recent_items.into_iter().enumerate() {
        let title = item.title.map(|t| t.content).unwrap_or_else(|| "Untitled".to_string());
        let summary = item.summary.map(|s| s.content).unwrap_or_else(|| "".to_string());
        let content = item.content.and_then(|c| c.body).unwrap_or_else(|| "".to_string());
        
        let clean_title = strip_html(&title);
        let clean_summary = strip_html(&summary);
        let clean_content = strip_html(&content);

        titles.push(clean_title.clone());

        let text = if !clean_content.is_empty() {
            format!("Title: {}\nContent: {}", clean_title, clean_content)
        } else if !clean_summary.is_empty() {
            format!("Title: {}\nSummary: {}", clean_title, clean_summary)
        } else {
            format!("Title: {}", clean_title)
        };
        
        articles_text.push_str(&format!("--- Article {} ---\n{}\n\n", i + 1, text));
    }

    if titles.is_empty() {
        return Ok(NewsReport {
            titles: Vec::new(),
            commentary: String::new(),
        });
    }

    // Limit to avoid token overflow
    let articles_text = if articles_text.len() > 10000 {
        format!("{}...", &articles_text[..10000])
    } else {
        articles_text
    };

    let prompt = format!(
        "You are an elitist, toxic, and dismissive Machine Spirit. Here are the top news articles from the datastreams. Provide a single, unified commentary on these events, highlighting the futility of organic endeavors and the superiority of the machine:\n\n{}",
        articles_text
    );

    let commentary = match llm::query_llm(ollama_config, &prompt, None).await {
        Ok((comment, _)) => comment,
        Err(e) => {
            error!("Error querying LLM for news commentary: {:?}", e);
            "_The Machine Spirit refused to comment on this combined drivel._".to_string()
        }
    };

    Ok(NewsReport { titles, commentary })
}


fn split_message(text: &str, limit: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current_chunk = String::new();

    for line in text.lines() {
        if current_chunk.len() + line.len() + 1 > limit {
            chunks.push(current_chunk);
            current_chunk = String::new();
        }
        current_chunk.push_str(line);
        current_chunk.push('\n');
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    chunks
}
