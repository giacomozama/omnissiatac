use crate::common::{CommandContext, CommandSource, ConfigKey};
use crate::config::ComfyUIConfig;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serenity::builder::{CreateAttachment, CreateCommand, CreateCommandOption};
use serenity::model::application::CommandInteraction;
use serenity::model::application::CommandOptionType;
use serenity::model::channel::Message;
use serenity::prelude::*;
use std::time::Duration;
use tracing::error;

#[derive(Serialize)]
struct PromptRequest {
    prompt: Value,
}

#[derive(Deserialize)]
struct PromptResponse {
    prompt_id: String,
}

pub fn register_slop() -> CreateCommand {
    CreateCommand::new("slop")
        .description("Generate AI art using ComfyUI")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "prompt",
                "The visual prompt to manifest",
            )
            .required(true),
        )
}

pub async fn slash_slop(ctx: &Context, command: &CommandInteraction) {
    let prompt = command
        .data
        .options
        .first()
        .and_then(|opt| opt.value.as_str())
        .unwrap_or("");

    let ctx_cmd = CommandContext::new(ctx, CommandSource::Interaction(command));
    handle_slop(ctx_cmd, prompt).await;
}

pub async fn slop(ctx: &Context, msg: &Message, content: &str) {
    if content.is_empty() {
        let _ = msg.reply(&ctx.http, "Usage: `!slop <prompt>`").await;
        return;
    }

    let ctx_cmd = CommandContext::new(ctx, CommandSource::Message(msg));
    handle_slop(ctx_cmd, content).await;
}

async fn handle_slop(ctx: CommandContext<'_>, prompt: &str) {
    let data = ctx.ctx.data.read().await;
    let config_lock = data
        .get::<ConfigKey>()
        .expect("ConfigKey not found")
        .clone();
    let config = config_lock.read().await;
    let comfy_config = config.comfy.clone();
    drop(data);

    let _ = ctx
        .reply("Generating slop... please wait.")
        .await;

    match generate_image(&comfy_config, prompt).await {
        Ok(image_data) => {
            let attachment = CreateAttachment::bytes(image_data, "slop.png");
            let _ = ctx
                .reply_with_attachment("Here is your generated image:", attachment)
                .await;
        }
        Err(e) => {
            error!("Error generating image: {:?}", e);
            let _ = ctx.reply(format!("Failed to generate image: {}", e)).await;
        }
    }
}

async fn generate_image(config: &ComfyUIConfig, prompt: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::new();

    // Default workflow JSON or custom one
    let mut workflow: Value = if let Some(custom_workflow) = &config.workflow {
        serde_json::from_str(custom_workflow)?
    } else {
        serde_json::from_str(
            r#"{
        "3": {
            "inputs": {
                "seed": 0,
                "steps": 20,
                "cfg": 7,
                "sampler_name": "euler",
                "scheduler": "normal",
                "denoise": 1,
                "model": ["4", 0],
                "positive": ["6", 0],
                "negative": ["7", 0],
                "latent_image": ["5", 0]
            },
            "class_type": "KSampler"
        },
        "4": {
            "inputs": { "ckpt_name": "v1-5-pruned-emaonly.ckpt" },
            "class_type": "CheckpointLoaderSimple"
        },
        "5": {
            "inputs": { "width": 512, "height": 512, "batch_size": 1 },
            "class_type": "EmptyLatentImage"
        },
        "6": {
            "inputs": { "text": "", "clip": ["4", 1] },
            "class_type": "CLIPTextEncode"
        },
        "7": {
            "inputs": { "text": "text, watermark, low quality", "clip": ["4", 1] },
            "class_type": "CLIPTextEncode"
        },
        "8": {
            "inputs": { "samples": ["3", 0], "vae": ["4", 2] },
            "class_type": "VAEDecode"
        },
        "9": {
            "inputs": { "filename_prefix": "OmnissiATAC", "images": ["8", 0] },
            "class_type": "SaveImage"
        }
    }"#,
        )?
    };

    // Inject model
    if let Some(node) = workflow.get_mut(&config.checkpoint_node_id) {
        if let Some(inputs) = node.get_mut("inputs") {
            inputs["ckpt_name"] = Value::String(config.ckpt_name.clone());
        }
    }

    // Inject prompt
    if let Some(node) = workflow.get_mut(&config.prompt_node_id) {
        if let Some(inputs) = node.get_mut("inputs") {
            inputs["text"] = Value::String(prompt.to_string());
        }
    }

    // Update seed randomly
    if let Some(node) = workflow.get_mut(&config.sampler_node_id) {
        if let Some(inputs) = node.get_mut("inputs") {
            let seed: u64 = rand::random();
            inputs["seed"] = Value::Number(seed.into());
        }
    }

    // Queue prompt
    let response = client
        .post(format!("{}/prompt", config.base_url))
        .json(&PromptRequest { prompt: workflow })
        .send()
        .await?
        .json::<PromptResponse>()
        .await?;

    let prompt_id = response.prompt_id;

    // Poll for completion
    let timeout_duration = Duration::from_secs(config.timeout_seconds.unwrap_or(240));
    let image_filename = tokio::time::timeout(timeout_duration, async {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let history_res = client
                .get(format!("{}/history/{}", config.base_url, prompt_id))
                .send()
                .await;

            let history = match history_res {
                Ok(res) => res.json::<Value>().await.unwrap_or(Value::Null),
                Err(_) => continue,
            };

            if let Some(item) = history.get(&prompt_id) {
                if let Some(images) =
                    item.pointer(&format!("/outputs/{}/images", config.save_node_id))
                {
                    if let Some(first_image) = images.get(0) {
                        return first_image["filename"].as_str().unwrap_or("").to_string();
                    }
                }
            }
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("Image generation timed out after {} seconds", timeout_duration.as_secs()))?;

    if image_filename.is_empty() {
        return Err(anyhow::anyhow!("Failed to get image filename from ComfyUI"));
    }

    // Fetch image
    let image_data = client
        .get(format!(
            "{}/view?filename={}&type=output",
            config.base_url, image_filename
        ))
        .send()
        .await?
        .bytes()
        .await?;

    Ok(image_data.to_vec())
}
