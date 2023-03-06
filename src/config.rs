use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub discord: DiscordConfig,
    pub lavalink: LavalinkConfig,
}

#[derive(Debug, Deserialize)]
pub struct DiscordConfig {
    pub token: String,
    pub app_id: String,
    pub intents: u32,
}

#[derive(Debug, Deserialize)]
pub struct LavalinkConfig {
    pub host: String,
    pub port: u16,
    pub password: String,
}

impl Config {
    pub async fn new() -> Result<Self, String> {
        let config = tokio::fs::read("config.toml")
            .await
            .map_err(|_| "Failed to read config.toml")?;

        toml::from_slice::<Config>(&config).map_err(|_| String::from("Failed to parse config.toml"))
    }
}
