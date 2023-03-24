use serde_json::Value;

pub mod builder;

use crate::{
    client::{
        events::{self, Interaction, InteractionCallbackData},
        LavalinkClient,
    },
    commands::builder::{
        ApplicationCommand, ApplicationCommandOption, ApplicationCommandOptionChoice,
    },
    toulen::{get_download_url, get_players},
};

fn format_time(time: u64) -> String {
    let seconds = time % 60;
    let minutes = (time / 60) % 60;
    let hours = (time / 3600) % 24;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

pub async fn command_handler(
    interaction: &Interaction,
    manager: &mut LavalinkClient,
) -> Result<(), String> {
    match interaction.get_name() {
        Some("ts") => {
            if let Err(_) = interaction.ack(64).await {
                // println!("Error acknowledging interaction: {err:?}");
            }
        }
        _ => {
            if let Err(err) = interaction.ack(0).await {
                println!("Error acknowledging interaction: {err:?}");
            }
        }
    }

    match interaction.get_name() {
        Some("join") => join_channel(interaction, manager).await,
        Some("leave") => leave_channel(interaction, manager).await,
        Some("play") => play_track(interaction, manager).await,
        Some("pause") => pause_track(interaction, manager).await,
        Some("skip") => skip_track(interaction, manager).await,
        Some("ts") => toulen(interaction).await,
        _ => Ok(()),
    }
}

async fn toulen(interaction: &Interaction) -> Result<(), String> {
    if let Err(err) = interaction.ack(64).await {
        println!("Error acknowledging interaction: {err:?}");
    }

    let name = interaction
        .data
        .as_ref()
        .and_then(|d| d.options.as_ref())
        .and_then(|o| o.first())
        .and_then(|o| o.get_name());

    match name {
        Some("download") => ts_download(interaction).await,
        Some("leaderboard") => ts_leaderboard(interaction).await,
        _ => Err(String::from("unknown subcommand")),
    }
}

async fn ts_download(interaction: &Interaction) -> Result<(), String> {
    let url = get_download_url().await?;

    let url = url.replace(".msi.zip", ".msi");

    let embed = events::Embed::new()
        .set_title(String::from("ToulenSniffer Download"))
        .set_description(format!("[download]({url})"))
        .set_color(0xf17c00);

    interaction
        .create_message(InteractionCallbackData::new().add_embed(embed))
        .await?;

    Ok(())
}

async fn ts_leaderboard(interaction: &Interaction) -> Result<(), String> {
    let players = get_players().await?;

    let mut leaderboard = String::new();

    for player in players {
        let (name, level) = (player.user.name, player.level.unwrap_or(0));

        leaderboard.push_str(format!("**{name}** - lvl. `{level}`\n").as_str());
    }

    let embed = events::Embed::new()
        .set_title(String::from("ToulenSniffer Leaderboard"))
        .set_description(leaderboard)
        .set_color(0xf17c00);

    interaction
        .create_message(InteractionCallbackData::new().add_embed(embed))
        .await?;

    Ok(())
}

async fn join_channel(
    interaction: &Interaction,
    manager: &mut LavalinkClient,
) -> Result<(), String> {
    if interaction.member.voice.is_none() {
        interaction
            .create_message(InteractionCallbackData::new().set_content("musíš být v roomce"))
            .await?;
    } else {
        let guild_id = &interaction.guild_id;
        let channel_id = match interaction.member.get_voice_channel() {
            Some(channel_id) => channel_id,
            None => {
                interaction
                    .create_message(
                        InteractionCallbackData::new().set_content("musíš být v roomce"),
                    )
                    .await?;

                return Ok(());
            }
        };

        if let Some(player) = manager.get_player(guild_id) {
            let channel_id = &player.channel_id;

            interaction
                .create_message(
                    InteractionCallbackData::new()
                        .set_content(&format!("už je připojen v <#{channel_id}>")),
                )
                .await?;

            return Ok(());
        };

        manager.join(guild_id, channel_id)?;

        interaction
            .create_message(
                InteractionCallbackData::new()
                    .set_content(&format!("připojeno do <#{channel_id}>")),
            )
            .await?;
    }

    Ok(())
}

async fn leave_channel(
    interaction: &Interaction,
    manager: &mut LavalinkClient,
) -> Result<(), String> {
    let guild_id = &interaction.guild_id;
    let channel_id = match interaction.member.get_voice_channel() {
        Some(channel_id) => channel_id,
        None => {
            interaction
                .create_message(InteractionCallbackData::new().set_content("musíš být v roomce"))
                .await?;

            return Ok(());
        }
    };

    let player = match manager.get_player(guild_id) {
        Some(player) => player,
        None => {
            interaction
                .create_message(InteractionCallbackData::new().set_content("nic nehraje"))
                .await?;

            return Ok(());
        }
    };

    if player.channel_id != *channel_id {
        interaction
            .create_message(
                InteractionCallbackData::new().set_content("musíš být ve stejné roomce"),
            )
            .await?;

        return Ok(());
    }

    manager.destroy_player(guild_id)?;

    interaction
        .create_message(InteractionCallbackData::new().set_content("odpojeno"))
        .await?;

    Ok(())
}

async fn play_track(interaction: &Interaction, manager: &mut LavalinkClient) -> Result<(), String> {
    let guild_id = &interaction.guild_id;
    let channel_id = match interaction.member.get_voice_channel() {
        Some(channel_id) => channel_id,
        None => {
            interaction
                .create_message(InteractionCallbackData::new().set_content("musíš být v roomce"))
                .await?;

            return Ok(());
        }
    };

    let player = match manager.get_player_mut(guild_id) {
        Some(player) => player,
        None => manager.join(guild_id, channel_id)?,
    };

    if player.channel_id != *channel_id {
        interaction
            .create_message(
                InteractionCallbackData::new().set_content("musíš být ve stejné roomce"),
            )
            .await?;

        return Ok(());
    }

    let content = interaction.get_value("query").ok_or("missing query")?;

    let content = match content {
        Value::String(content) => content,
        _ => return Err(String::from("missing query")),
    };

    let platform = match interaction.get_value("platform") {
        Some(Value::String(platform)) => Some(platform.as_str()),
        _ => {
            if content.starts_with("https://") || content.starts_with("http://") {
                None
            } else {
                Some("ytsearch")
            }
        }
    };

    let result = player.search(content, platform).await?;

    if result.tracks.is_empty() {
        interaction
            .create_message(InteractionCallbackData::new().set_content("nic nebylo nenalezeno"))
            .await?;

        return Ok(());
    }

    if result.load_type == "PLAYLIST_LOADED" {
        let playlist_tracks_num = result.tracks.len();
        let playlist_len = result.tracks.iter().map(|t| t.info.length).sum::<u64>();

        let name = result
            .playlist_info
            .as_ref()
            .unwrap()
            .name
            .as_ref()
            .unwrap();

        let identifier = &result.tracks.first().unwrap().info.identifier;

        let lenght = format_time(playlist_len / 1000);

        let embed = events::Embed::new()
            .set_description(format!(
                "**[{name}]()**\npřidáno {playlist_tracks_num} songů do fronty" // result.playlist_info.unwrap().selected_track.unwrap()
            ))
            .set_color(0x0080f0)
            .set_thumbnail(events::EmbedThumbnail {
                url: Some(format!(
                    "https://i.ytimg.com/vi/{identifier}/maxresdefault.jpg"
                )),
                proxy_url: None,
                height: None,
                width: None,
            })
            .set_footer(events::EmbedFooter {
                text: format!("Trvání: {lenght}"),
                icon_url: None,
                proxy_icon_url: None,
            });

        interaction
            .create_message(InteractionCallbackData::new().add_embed(embed))
            .await?;

        for track in result.tracks {
            player.play(track);
        }
    } else if let Some(track) = result.tracks.into_iter().next() {
        let (title, uri) = (&track.info.title, &track.info.uri);

        let identifier = &track.info.identifier;

        let lenght = format_time(track.info.length / 1000);

        let embed = events::Embed::new()
            .set_description(format!("**[{title}]({uri})**\nbylo přidáno do fronty"))
            .set_color(0x0080f0)
            .set_thumbnail(events::EmbedThumbnail {
                url: Some(format!(
                    "https://i.ytimg.com/vi/{identifier}/maxresdefault.jpg"
                )),
                proxy_url: None,
                height: None,
                width: None,
            })
            .set_footer(events::EmbedFooter {
                text: format!("Trváni: {lenght}"),
                icon_url: None,
                proxy_icon_url: None,
            });

        interaction
            .create_message(InteractionCallbackData::new().add_embed(embed))
            .await?;

        player.play(track);
    }

    Ok(())
}

async fn pause_track(
    interaction: &Interaction,
    manager: &mut LavalinkClient,
) -> Result<(), String> {
    let guild_id = &interaction.guild_id;
    let channel_id = match interaction.member.get_voice_channel() {
        Some(channel_id) => channel_id,
        None => {
            interaction
                .create_message(InteractionCallbackData::new().set_content("musíš být v roomce"))
                .await?;

            return Ok(());
        }
    };

    let player = match manager.get_player_mut(guild_id) {
        Some(player) => player,
        None => {
            interaction
                .create_message(InteractionCallbackData::new().set_content("musíš být v roomce"))
                .await?;

            return Ok(());
        }
    };

    if player.channel_id != *channel_id {
        interaction
            .create_message(
                InteractionCallbackData::new().set_content("musíš být ve stejné roomce"),
            )
            .await?;

        return Ok(());
    }

    let paused = interaction.get_value("paused").ok_or("missing query")?;

    let paused = match paused {
        Value::Bool(paused) => paused,
        _ => return Err(String::from("missing query")),
    };

    player.pause(*paused);

    interaction
        .create_message(InteractionCallbackData::new().set_content(if *paused {
            "přehrávání pozastaveno"
        } else {
            "pokračování v přehrávání"
        }))
        .await?;

    Ok(())
}

async fn skip_track(interaction: &Interaction, manager: &mut LavalinkClient) -> Result<(), String> {
    let guild_id = &interaction.guild_id;
    let channel_id = match interaction.member.get_voice_channel() {
        Some(channel_id) => channel_id,
        None => {
            interaction
                .create_message(InteractionCallbackData::new().set_content("musíš být v roomce"))
                .await?;

            return Ok(());
        }
    };

    let player = match manager.get_player_mut(guild_id) {
        Some(player) => player,
        None => {
            interaction
                .create_message(InteractionCallbackData::new().set_content("musíš být v roomce"))
                .await?;

            return Ok(());
        }
    };

    if player.channel_id != *channel_id {
        interaction
            .create_message(
                InteractionCallbackData::new().set_content("musíš být ve stejné roomce"),
            )
            .await?;

        return Ok(());
    }

    let track = match player.skip() {
        Some(track) => track,
        None => {
            interaction
                .create_message(InteractionCallbackData::new().set_content("nic nehraje"))
                .await?;

            return Ok(());
        }
    };

    let (title, uri) = (&track.info.title, &track.info.uri);

    let identifier = &track.info.identifier;

    let embed = events::Embed::new()
        .set_description(format!("**[{title}]({uri})**\nbylo přeskočeno"))
        .set_color(0x0080f0)
        .set_thumbnail(events::EmbedThumbnail {
            url: Some(format!(
                "https://i.ytimg.com/vi/{identifier}/maxresdefault.jpg"
            )),
            proxy_url: None,
            height: None,
            width: None,
        });

    interaction
        .create_message(InteractionCallbackData::new().add_embed(embed))
        .await?;

    Ok(())
}
pub struct Commands {
    pub commands: Vec<ApplicationCommand>,
}

impl Commands {
    pub fn new() -> Self {
        let join_cmd = ApplicationCommand::new(
            1,
            String::from("join"),
            String::from("joins the voice channel"),
        )
        .set_name_loc("připojit")
        .set_desc_loc("připojí bota do roomky");

        let leave_cmd = ApplicationCommand::new(
            1,
            String::from("leave"),
            String::from("leaves the voice channel"),
        )
        .set_name_loc("odpojit")
        .set_desc_loc("odpojí bota z roomky");

        let mut play_cmd =
            ApplicationCommand::new(1, String::from("play"), String::from("plays a song"))
                .set_name_loc("hraj")
                .set_desc_loc("přehraje song");

        play_cmd.add_option(
            ApplicationCommandOption::new(
                String::from("query"),
                String::from("song to play"),
                3,
                true,
            )
            .set_name_loc("vyhledávání")
            .set_desc_loc("song k přehrávání"),
        );

        let mut platform_choice = ApplicationCommandOption::new(
            String::from("platform"),
            String::from("platform to search on"),
            3,
            false,
        )
        .set_name_loc("platforma")
        .set_desc_loc("platforma pro vyhledávání");

        platform_choice.add_choice(ApplicationCommandOptionChoice {
            name: String::from("YouTube"),
            value: String::from("ytsearch"),
        });

        platform_choice.add_choice(ApplicationCommandOptionChoice {
            name: String::from("YouTube Music"),
            value: String::from("ytmsearch"),
        });

        platform_choice.add_choice(ApplicationCommandOptionChoice {
            name: String::from("SoundCloud"),
            value: String::from("scsearch"),
        });

        play_cmd.add_option(platform_choice);

        let mut pause_cmd = ApplicationCommand::new(
            1,
            String::from("pause"),
            String::from("pauses the current song"),
        )
        .set_name_loc("pauza")
        .set_desc_loc("pauzuje přehrávání");

        pause_cmd.add_option(
            ApplicationCommandOption::new(
                String::from("paused"),
                String::from("pause or unpause"),
                5,
                true,
            )
            .set_name_loc("pauza")
            .set_desc_loc("pauza nebo pokračování"),
        );

        let skip_cmd = ApplicationCommand::new(
            1,
            String::from("skip"),
            String::from("skips the current song"),
        )
        .set_name_loc("přeskočit")
        .set_desc_loc("přeskočí song");

        Self {
            commands: vec![join_cmd, leave_cmd, play_cmd, pause_cmd, skip_cmd],
        }
    }

    pub fn toulen() -> Self {
        let mut ts_cmds = ApplicationCommand::new(
            1,
            String::from("ts"),
            String::from("ToulenSniffer comannnds"),
        )
        .set_desc_loc("ToulenSniffer commandy");

        ts_cmds.add_option(
            ApplicationCommandOption::new(
                String::from("leaderboard"),
                String::from("shows top players"),
                1,
                false,
            )
            .set_name_loc("žebříčky")
            .set_desc_loc("zobrazí top hráče"),
        );

        ts_cmds.add_option(
            ApplicationCommandOption::new(
                String::from("download"),
                String::from("ToulenSniffer download"),
                1,
                false,
            )
            .set_name_loc("stáhnout")
            .set_desc_loc("odkaz na stažení ToulenSniffer"),
        );

        Self {
            commands: vec![ts_cmds],
        }
    }
}
