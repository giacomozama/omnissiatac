use crate::config::OllamaConfig;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    message: ChatMessage,
}

pub async fn query_llm(
    config: &OllamaConfig,
    prompt: &str,
    history: Option<Vec<ChatMessage>>,
) -> Result<(String, Vec<ChatMessage>)> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/chat", config.base_url);

    let mut messages = if let Some(h) = history {
        h
    } else {
        let system_content = config.system_prompt.clone().unwrap_or("".to_owned());
        vec![ChatMessage {
            role: "system".to_string(),
            content: system_content,
        }]
    };

    messages.push(ChatMessage {
        role: "user".to_string(),
        content: prompt.to_string(),
    });

    let request = ChatRequest {
        model: config.model.clone(),
        messages: messages.clone(),
        stream: false,
    };

    let mut rb = client.post(&url).json(&request);

    if let Some(ref key) = config.api_key {
        if !key.is_empty() {
            rb = rb.header("Authorization", format!("Bearer {}", key));
        }
    }

    let response = rb.send().await?.json::<ChatResponse>().await?;

    let assistant_message = response.message;
    messages.push(assistant_message.clone());

    Ok((assistant_message.content, messages))
}
