use std::env;

use franta_rust::{
    client::{Client, ClientBuilderOptions, LavalinkBuilderOptions},
    commands::Commands,
    config::Config,
};

#[tokio::main]
async fn main() {
    let config = Config::new().await.unwrap();

    let client = Client::new(
        ClientBuilderOptions {
            intents: config.discord.intents,
            token: config.discord.token,
            app_id: config.discord.app_id,
        },
        LavalinkBuilderOptions {
            host: config.lavalink.host,
            port: config.lavalink.port,
            password: config.lavalink.password,
        },
    );

    let commands = Commands::new();
    let ts_commdands = Commands::toulen();

    match env::args().nth(1).as_deref() {
        Some("toulen") => {
            for command in ts_commdands.commands {
                if let Err(err) = client
                    .add_guild_command(String::from("456060911573008385"), command)
                    .await
                {
                    println!("Error while adding guild command: {err}");
                }
            }

            panic!("Done adding commands");
        }
        Some("global") => {
            for command in commands.commands {
                if let Err(err) = client.add_command(command).await {
                    println!("Error while adding guild command: {err}");
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }

            panic!("Done adding commands");
        }
        _ => {}
    }

    client.login().await.unwrap();
}
