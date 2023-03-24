use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use event_loop::DiscordEvLoop;
use events::Event;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc::{self, UnboundedSender};

use crate::client::events::InteractionCallbackData;
use crate::commands::{builder::ApplicationCommand, command_handler};

use self::event_loop::{LavalinkEvLoop, ResumeProperties};
use self::events::{VoiceServer, VoiceState};

mod event_handler;
mod event_loop;
pub mod events;

pub struct Client {
    socket: DiscordEvLoop,
    options: ClientBuilderOptions,
    manager: LavalinkClient,
    voice_states: Vec<Arc<VoiceState>>,
    user: String,
}

pub struct ClientBuilderOptions {
    pub intents: u32,
    pub token: String,
    pub app_id: String,
}

impl Client {
    pub fn new(options: ClientBuilderOptions, lavalink_options: LavalinkBuilderOptions) -> Self {
        let ws_stream = DiscordEvLoop::new();

        Self {
            socket: ws_stream,
            options,
            manager: LavalinkClient::new(lavalink_options),
            voice_states: Vec::new(),
            user: String::new(),
        }
    }
}

impl Client {
    pub async fn login(mut self) -> Result<(), String> {
        let (tx, mut rx) = mpsc::unbounded_channel::<Event>();

        let tx = Arc::new(tx);

        self.socket
            .create_handles(Arc::clone(&tx), "wss://gateway.discord.gg", None)
            .await
            .map_err(|_| "Error connecting to the gateway")?;

        let payload = json!({
            "op": 2,
            "d": {
                "token": self.options.token,
                "intents": self.options.intents,
                "properties": {
                    "os": "linux",
                    "browser": "franta-rust",
                    "device": "franta-rust"
                }
            }
        })
        .to_string();

        self.socket
            .send(&payload)
            .map_err(|_| "Error sending login payload")?;

        self.manager.tx = Some(Arc::clone(&tx));

        self.manager.connect().await?;

        let mut socket = self.socket;

        let mut resume_props = ResumeProperties {
            token: self.options.token,
            session_id: String::new(),
            seq: 0,
            resume_gateway_url: String::new(),
        };

        let mut players_to_destroy = Vec::new();

        while let Some(event) = rx.recv().await {
            match event {
                Event::ResumeSeq(seq_id) => resume_props.seq = seq_id,

                Event::ResumeProps((gateway_url, session_id)) => {
                    resume_props.resume_gateway_url = gateway_url;
                    resume_props.session_id = session_id;
                }

                Event::Resume => {
                    socket.abort_tasks();

                    socket
                        .create_handles(
                            Arc::clone(&tx),
                            &resume_props.resume_gateway_url,
                            Some(&resume_props),
                        )
                        .await
                        .map_err(|_| "Error resuming connection to the gateway")?;
                }

                Event::Reconnect => {
                    println!("Reconnecting...");

                    socket.abort_tasks();

                    socket
                        .create_handles(Arc::clone(&tx), &resume_props.resume_gateway_url, None)
                        .await
                        .map_err(|_| "Error reconnecting to the gateway")?;

                    socket
                        .send(&payload)
                        .map_err(|_| "Error sending login payload")?;
                }

                Event::LavalinkClosed => {
                    println!("Lavalink connection closed, attempting to resume");

                    self.manager.socket.abort_tasks();

                    self.manager.connect().await?;

                    self.manager.update_player_tx();
                }

                Event::Ready(user) => {
                    let (username, discriminator) = (user.username, user.discriminator);

                    println!("{username}#{discriminator} has logged in!");

                    self.user = user.id;
                }

                Event::SendWS(payload) => {
                    if let Err(err) = socket.send(&payload) {
                        println!("Error sending payload: {err}");
                    };
                }

                Event::VoiceStateUpdate(voice_state) => {
                    let voice_state = Arc::new(voice_state);

                    if voice_state.user_id == self.user {
                        if voice_state.channel_id.is_none() {
                            // self.manager.voice_states.remove(&voice_state.guild_id);
                            self.manager
                                .voice_states
                                .retain(|state| state.guild_id != voice_state.guild_id);

                            if let Err(err) = self.manager.destroy_player(&voice_state.guild_id) {
                                println!("Error destroying player: {err:?}");
                            };
                        } else {
                            // let guild_id = voice_state.guild_id.to_string();

                            // self.manager
                            //     .voice_states
                            //     .insert(guild_id.to_string(), voice_state.session_id.to_string());

                            match self
                                .manager
                                .voice_states
                                .iter_mut()
                                .find(|state| state.guild_id == voice_state.guild_id)
                            {
                                Some(state) => {
                                    *state = Arc::clone(&voice_state);
                                }
                                None => {
                                    self.manager.voice_states.push(Arc::clone(&voice_state));
                                }
                            }

                            if let Some(player) = self.manager.get_player_mut(&voice_state.guild_id)
                            {
                                player.channel_id = voice_state.channel_id.clone().unwrap();
                            }

                            // println!("Attempting to connect to voice channel...");

                            // if let Err(err) = self.manager.attempt_connection(guild_id) {
                            //     println!("Error connecting to voice channel: {:?}", err);
                            // };
                        }
                    }

                    if voice_state.channel_id.is_none() {
                        let prev_voice_state = &self.voice_states.iter().find(|state| {
                            state.guild_id == voice_state.guild_id
                                && state.user_id == voice_state.user_id
                        });

                        if let Some(_) = self.voice_states.iter().find(|state| {
                            state.guild_id == voice_state.guild_id && state.user_id == self.user
                        }) {
                            if prev_voice_state.is_some()
                                && self
                                    .voice_states
                                    .iter()
                                    .filter(|state| {
                                        state.guild_id == voice_state.guild_id
                                            && state.channel_id
                                                == prev_voice_state.unwrap().channel_id
                                    })
                                    .count()
                                    == 2
                            {
                                let tx_c = Arc::clone(&tx);
                                let guild_id = voice_state.guild_id.clone();

                                let handle = tokio::spawn(async move {
                                    tokio::time::sleep(Duration::from_secs(300)).await;

                                    if let Err(err) = tx_c.send(Event::DestroyPlayer(guild_id)) {
                                        println!("Error sending destroy player event: {err}");
                                    };
                                });

                                players_to_destroy.push((voice_state.guild_id.clone(), handle));
                            }
                        }

                        self.voice_states.retain(|state| {
                            if state.user_id != voice_state.user_id {
                                return true;
                            }

                            state.guild_id != voice_state.guild_id
                        });

                        // self.voice_states
                        //     .remove(&(voice_state.guild_id, voice_state.user_id));
                    } else {
                        if let Some(_) = self.voice_states.iter().find(|state| {
                            state.guild_id == voice_state.guild_id && state.user_id == self.user
                        }) {
                            if self
                                .voice_states
                                .iter()
                                .filter(|state| {
                                    state.guild_id == voice_state.guild_id
                                        && state.channel_id == voice_state.channel_id
                                })
                                .count()
                                == 1
                            {
                                if let Some((_, handle)) = players_to_destroy
                                    .iter()
                                    .find(|(guild_id, _)| *guild_id == voice_state.guild_id)
                                {
                                    handle.abort();

                                    players_to_destroy
                                        .retain(|(guild_id, _)| *guild_id != voice_state.guild_id);
                                }
                            }
                        }

                        match self.voice_states.iter_mut().find(|state| {
                            state.guild_id == voice_state.guild_id
                                && state.user_id == voice_state.user_id
                        }) {
                            Some(state) => {
                                *state = Arc::clone(&voice_state);
                            }
                            None => {
                                self.voice_states.push(Arc::clone(&voice_state));
                            }
                        }

                        // self.voice_states.insert(
                        //     (
                        //         voice_state.guild_id.to_string(),
                        //         voice_state.user_id.to_string(),
                        //     ),
                        //     Arc::new(voice_state),
                        // );
                    }
                }

                Event::VoiceServerUpdate(voice_server) => {
                    // let guild_id = voice_server.guild_id.to_string();

                    // self.manager
                    //     .voice_servers
                    //     .insert(guild_id.to_string(), voice_server);

                    let voice_server = Rc::new(voice_server);

                    match self
                        .manager
                        .voice_servers
                        .iter_mut()
                        .find(|server| server.guild_id == voice_server.guild_id)
                    {
                        Some(server) => {
                            *server = Rc::clone(&voice_server);
                        }
                        None => {
                            self.manager.voice_servers.push(Rc::clone(&voice_server));
                        }
                    }

                    if let Err(err) = self.manager.attempt_connection(&voice_server.guild_id) {
                        println!("Error connecting to voice channel: {err:?}");
                    };
                }

                Event::TrackEnd(guild_id) => {
                    if let Some(player) = self.manager.get_player_mut(&guild_id) {
                        player.queue.remove(0);

                        if !player.queue.is_empty() {
                            let track = &player.queue[0];

                            player.send(
                                json!({
                                    "op": "play",
                                    "guildId": guild_id,
                                    "track": track.track
                                })
                                .to_string(),
                            );
                        } else {
                            player.playing = false;
                        }
                    }
                }

                Event::InteractionCreate(interaction) => {
                    if let Some(voice_state) = /*self.voice_states.get(&(
                            interaction.guild_id.to_string(),
                            interaction.member.user.id.to_string(),
                        ))*/
                        self.voice_states.iter().find(|state| {
                            state.guild_id == interaction.guild_id
                                && state.user_id == interaction.member.user.id
                        })
                    {
                        let interaction = interaction.update_voice(Arc::clone(voice_state));

                        if let Err(err) = command_handler(&interaction, &mut self.manager).await {
                            println!("Error handling command: {err:?}");

                            if let Err(err) = interaction
                                .create_message(
                                    InteractionCallbackData::new().set_content("něco se pokazilo"),
                                )
                                .await
                            {
                                println!("Error sending message: {err:?}");
                            }
                        };
                    } else if let Err(err) = command_handler(&interaction, &mut self.manager).await
                    {
                        println!("Error handling command: {err:?}");

                        if let Err(err) = interaction
                            .create_message(
                                InteractionCallbackData::new().set_content("něco se pokazilo"),
                            )
                            .await
                        {
                            println!("Error sending message: {err:?}");
                        }
                    };
                }

                Event::DestroyPlayer(guild_id) => {
                    if let Err(err) = self.manager.destroy_player(&guild_id) {
                        println!("Error destroying player: {err:?}");
                    };

                    players_to_destroy.retain(|(guild_id, _)| guild_id != guild_id);
                }
            }
        }

        Ok(())
    }

    pub async fn add_command(&self, command: ApplicationCommand) -> Result<(), String> {
        let app_id = &self.options.app_id;

        let url = format!("https://discord.com/api/v10/applications/{app_id}/commands");

        let client = reqwest::Client::new();

        let token = &self.options.token;

        let res = client
            .post(url)
            .header("Authorization", format!("Bot {token}"))
            .json(&command)
            .send()
            .await
            .map_err(|_| "Error adding command")?;

        if res.status().is_success() {
            Ok(())
        } else {
            let status = res.status();

            Err(format!("Err: {status}, while adding command {command:#?}"))
        }
    }

    pub async fn add_guild_command(
        &self,
        guild_id: String,
        command: ApplicationCommand,
    ) -> Result<(), String> {
        let app_id = &self.options.app_id;

        let url =
            format!("https://discord.com/api/v10/applications/{app_id}/guilds/{guild_id}/commands");

        let client = reqwest::Client::new();

        let token = &self.options.token;

        let res = client
            .post(url)
            .header("Authorization", format!("Bot {token}"))
            .json(&command)
            .send()
            .await
            .map_err(|_| "Error adding command")?;

        if res.status().is_success() {
            Ok(())
        } else {
            let status = res.status();

            Err(format!("Err: {status}, while adding command {command:#?}"))
        }
    }
}

#[derive(Debug)]
pub struct LavalinkClient {
    socket: LavalinkEvLoop,
    tx: Option<Arc<UnboundedSender<Event>>>,
    voice_servers: Vec<Rc<VoiceServer>>,
    voice_states: Vec<Arc<VoiceState>>,
    players: Vec<Player>,
}

#[derive(Debug)]
pub struct LavalinkBuilderOptions {
    pub host: String,
    pub port: u16,
    pub password: String,
}

impl LavalinkClient {
    pub fn new(options: LavalinkBuilderOptions) -> LavalinkClient {
        let ws_stream = LavalinkEvLoop::new(options);

        LavalinkClient {
            socket: ws_stream,
            tx: None,
            voice_servers: Vec::new(),
            voice_states: Vec::new(),
            players: vec![],
        }
    }

    pub async fn connect(&mut self) -> Result<(), String> {
        let tx = self.tx.as_ref().ok_or("missing sender")?;

        self.socket.connect(Arc::clone(tx)).await
    }

    pub fn join(&mut self, guild_id: &String, channel_id: &String) -> Result<&mut Player, String> {
        if self.players.iter().any(|p| p.guild_id == *guild_id) {
            return Err(format!("Already in a voice channel in {guild_id}"));
        }

        self.send_ws(guild_id, Some(channel_id.to_string()))?;

        let sender = self.socket.sender.as_ref().ok_or("missing sender")?;

        self.players.push(Player::new(
            guild_id.to_string(),
            channel_id.to_string(),
            Arc::clone(sender),
            Rc::clone(&self.socket.options),
        ));

        let player = self.get_player_mut(guild_id).ok_or("Player not found")?;

        Ok(player)
    }

    pub fn destroy_player(&mut self, guild_id: &String) -> Result<(), String> {
        self.send_ws(guild_id, None)?;

        let player = self.get_player(guild_id).ok_or("Player not found")?;

        player.send(
            json!({
                "op": "destroy",
                "guildId": guild_id
            })
            .to_string(),
        );

        self.players.retain(|p| p.guild_id != *guild_id);

        Ok(())
    }

    pub fn send_ws(&self, guild_id: &String, channel_id: Option<String>) -> Result<(), String> {
        let payload = json!({
            "op": 4,
            "d": {
                "guild_id": guild_id,
                "channel_id": channel_id,
                "self_mute": false,
                "self_deaf": false
            }
        })
        .to_string();

        if let Some(tx) = &self.tx {
            tx.send(Event::SendWS(payload))
                .map_err(|_| "Error sending payload")?;
        }

        Ok(())
    }

    pub fn get_player(&self, guild_id: &str) -> Option<&Player> {
        self.players
            .iter()
            .find(|player| player.guild_id == *guild_id)
    }

    pub fn get_player_mut(&mut self, guild_id: &str) -> Option<&mut Player> {
        self.players
            .iter_mut()
            .find(|player| player.guild_id == *guild_id)
    }

    fn attempt_connection(&self, guild_id: &str) -> Result<(), String> {
        let server = match /*self.voice_servers.get(&guild_id)*/self.voice_servers.iter().find(|s| s.guild_id == guild_id) {
            Some(server) => server,
            None => return Err(String::from("No voice server found")),
        };

        let session = match /*self.voice_states.get(&guild_id)*/self.voice_states.iter().find(|s| s.guild_id == guild_id) {
            Some(session) => &session.session_id,
            None => return Err(String::from("No voice state found")),
        };

        let player = match self.get_player(&guild_id) {
            Some(player) => player,
            None => return Err(String::from("No player found")),
        };

        player.connect(session, server);

        Ok(())
    }

    fn update_player_tx(&mut self) {
        if let Some(sender) = &self.socket.sender {
            for player in &mut self.players {
                player.tx = Arc::clone(sender);
            }
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SearchResult {
    #[serde(rename = "loadType")]
    pub load_type: String,
    #[serde(rename = "playlistInfo")]
    pub playlist_info: Option<PlaylistInfo>,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Track {
    pub track: String,
    pub info: TrackInfo,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TrackInfo {
    pub identifier: String,
    pub author: String,
    pub length: u64,
    pub position: u64,
    pub title: String,
    pub uri: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PlaylistInfo {
    pub name: Option<String>,
    #[serde(rename = "selectedTrack")]
    pub selected_track: Option<i32>,
}

#[derive(Debug)]
pub struct Player {
    pub guild_id: String,
    pub channel_id: String,
    pub paused: bool,
    pub volume: u8,
    pub playing: bool,
    pub queue: Vec<Track>,
    tx: Arc<UnboundedSender<String>>,
    options: Rc<LavalinkBuilderOptions>,
}

impl Player {
    pub fn new(
        guild_id: String,
        channel_id: String,
        tx: Arc<UnboundedSender<String>>,
        options: Rc<LavalinkBuilderOptions>,
    ) -> Self {
        Self {
            guild_id,
            channel_id,
            paused: false,
            volume: 100,
            playing: false,
            queue: vec![],
            tx,
            options,
        }
    }

    pub fn connect(&self, session_id: &str, event: &VoiceServer) {
        self.send(
            json!({
                "op": "voiceUpdate",
                "guildId": self.guild_id,
                "sessionId": session_id,
                "event": event
            })
            .to_string(),
        );
    }

    pub fn send(&self, payload: String) {
        if let Err(err) = self.tx.send(payload) {
            println!("Error sending payload to lavalink: {err}");
        }
    }

    pub async fn search(
        &self,
        query: &str,
        platform: Option<&str>,
    ) -> Result<SearchResult, String> {
        let query_string = match platform {
            Some(platform) => format!("{platform}:{query}"),
            None => query.to_string(),
        };

        let client = reqwest::Client::new();

        let (host, port) = (&self.options.host, self.options.port);

        let url = format!("http://{host}:{port}/loadtracks?identifier={query_string}");

        let res = client
            .get(url)
            .header("Authorization", &self.options.password)
            .send()
            .await
            .map_err(|_| "Error searching")?
            .json()
            .await
            .map_err(|e| format!("Error parsing search result: {e:?}"))?;

        Ok(res)
    }

    pub fn play(&mut self, track: Track) {
        if !self.playing {
            self.playing = true;

            self.send(
                json!({
                    "op": "play",
                    "guildId": self.guild_id,
                    "track": track.track
                })
                .to_string(),
            );
        }

        self.queue.push(track);
    }

    pub fn pause(&self, paused: bool) {
        self.send(
            json!({
                "op": "pause",
                "guildId": self.guild_id,
                "pause": paused
            })
            .to_string(),
        );
    }

    pub fn skip(&self) -> Option<&Track> {
        if self.queue.is_empty() {
            return None;
        }

        let track = self.queue.first();

        self.send(
            json!({
                "op": "stop",
                "guildId": self.guild_id
            })
            .to_string(),
        );

        track
    }
}
