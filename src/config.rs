use anyhow::Result;
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub discord: DiscordConfig,
    pub lavalink: LavalinkConfig,
    pub bot: BotConfig,
    pub ollama: OllamaConfig,
    pub comfy: ComfyUIConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DiscordConfig {
    pub token: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LavalinkConfig {
    pub host: String,
    pub port: u16,
    pub password: String,
    pub is_ssl: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BotConfig {
    pub inactivity_timeout_seconds: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OllamaConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ComfyUIConfig {
    pub base_url: String,
    pub ckpt_name: String,
    pub checkpoint_node_id: String,
    pub prompt_node_id: String,
    pub sampler_node_id: String,
    pub save_node_id: String,
    pub workflow: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Path::new("config.toml");
        if !config_path.exists() {
            return Err(anyhow::anyhow!("config.toml not found"));
        }

        let content = fs::read_to_string(config_path)?;
        let config: Config = toml::from_str(&content)?;

        // Ensure password.hash file exists with default if not present
        if !Path::new("password.hash").exists() {
            Self::set_password("password")?;
        }

        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write("config.toml", content)?;
        Ok(())
    }

    pub fn verify_password(password: &str) -> bool {
        let hash = match fs::read_to_string("password.hash") {
            Ok(h) => h,
            Err(_) => return false,
        };

        let parsed_hash = match PasswordHash::new(&hash) {
            Ok(p) => p,
            Err(_) => return false,
        };

        Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok()
    }

    pub fn set_password(password: &str) -> Result<()> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("failed to hash password: {}", e))?
            .to_string();

        fs::write("password.hash", password_hash)?;
        Ok(())
    }
}
