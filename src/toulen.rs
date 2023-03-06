use reqwest::Client;
use serde::Deserialize;

pub async fn get_download_url() -> Result<String, String> {
    let res = Client::new().get("https://gist.githubusercontent.com/gorgbus/64160457f8144af815a94eca5fbc6be7/raw/5b9f28a4e9f76382685a1bbc2bda15d1ad8ecb60/meta.json").send().await.map_err(|_| String::from("Failed to get download url"))?;

    if res.status().is_success() {
        let version_info: VersionInfo = res
            .json()
            .await
            .map_err(|_| String::from("Failed to get download url"))?;

        Ok(version_info.platforms.windows.url)
    } else {
        let status = res.status();

        Err(format!("Failed to get download url: {status}"))
    }
}

pub async fn get_players() -> Result<Vec<Player>, String> {
    let res = Client::new()
        .get("https://toulen.onrender.com/api/players")
        .send()
        .await
        .map_err(|_| String::from("Failed to get players"))?;

    if res.status().is_success() {
        let players: Vec<Player> = res
            .json()
            .await
            .map_err(|_| String::from("Failed to get players"))?;

        let players = players
            .into_iter()
            .map(|mut player| {
                player.level = Some(player.auto_lvl + player.regen_lvl + player.stamina_lvl);

                player
            })
            .collect();

        Ok(players)
    } else {
        let status = res.status();

        Err(format!("Failed to get players: {status}"))
    }
}

#[derive(Debug, Deserialize)]
pub struct Player {
    auto_lvl: u32,
    regen_lvl: u32,
    stamina_lvl: u32,
    pub user: User,
    pub level: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub name: String,
}

#[derive(Debug, Deserialize)]
struct VersionInfo {
    platforms: Platforms,
}

#[derive(Debug, Deserialize)]
struct Platforms {
    #[serde(rename = "windows-x86_64")]
    windows: Windows,
}

#[derive(Debug, Deserialize)]
struct Windows {
    url: String,
}
