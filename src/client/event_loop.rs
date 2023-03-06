use std::rc::Rc;
use std::sync::Arc;

use crate::client::{Event, LavalinkBuilderOptions};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use http::Request;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio::time::{self, Duration};
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::CloseFrame, tungstenite::Message, MaybeTlsStream,
    WebSocketStream,
};

type WsStreamType = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type Write = SplitSink<WsStreamType, Message>;
type Read = SplitStream<WsStreamType>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResumeProperties {
    pub resume_gateway_url: String,
    pub session_id: String,
    pub seq: u64,
    pub token: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct GatewayResume {
    op: u8,
    d: ResumeProperties,
}

async fn get_heartbeat(ws_stream: &mut WsStreamType) -> Result<u64, ()> {
    if let Some(Ok(Message::Text(msg))) = ws_stream.next().await {
        let msg: Value = serde_json::from_str(&msg).map_err(|_| ())?;

        let interval = msg
            .get("d")
            .ok_or(())?
            .get("heartbeat_interval")
            .ok_or(())?
            .as_u64()
            .ok_or(())?;

        return Ok(interval);
    }

    Err(())
}

fn create_event(event_name: &str, value: &Value) -> Result<Event, String> {
    let err_get = format!("Failed to create {event_name} event: Failed to get data");
    let err_parse = format!("Failed to create {event_name} event: Failed to parse data");

    match event_name {
        "READY" => {
            let user = value
                .get("d")
                .ok_or(&err_get)?
                .get("user")
                .ok_or(&err_get)?;

            let event = serde_json::from_value(user.to_owned()).map_err(|_| err_parse)?;

            Ok(Event::Ready(event))
        }

        "INTERACTION_CREATE" => {
            let interaction = value.get("d").ok_or(err_get)?;

            let event = serde_json::from_value(interaction.to_owned()).map_err(|_| err_parse)?;

            Ok(Event::InteractionCreate(event))
        }

        "VOICE_STATE_UPDATE" => {
            let voice_state = value.get("d").ok_or(err_get)?;

            let event = serde_json::from_value(voice_state.to_owned()).map_err(|_| err_parse)?;

            Ok(Event::VoiceStateUpdate(event))
        }

        "VOICE_SERVER_UPDATE" => {
            let voice_server = value.get("d").ok_or(err_get)?;

            let event = serde_json::from_value(voice_server.to_owned()).map_err(|_| err_parse)?;

            Ok(Event::VoiceServerUpdate(event))
        }

        _ => Err(format!("Event {event_name} was not found")),
    }
}

fn on_close(close: Option<CloseFrame>) -> Result<Event, String> {
    let resume = Event::Resume;
    let reconnect = Event::Reconnect;

    let close = close.ok_or("Closed with no close code")?;

    match close.code {
        CloseCode::Library(code) => match code {
            4000 => Ok(resume),
            4001 => Ok(resume),
            4002 => Ok(resume),

            4003 => Ok(reconnect),
            4005 => Ok(reconnect),
            4007 => Ok(reconnect),
            4008 => Ok(reconnect),

            4009 => Ok(resume),

            4004 => Err(close.reason.to_string()),
            4010 => Err(close.reason.to_string()),
            4011 => Err(close.reason.to_string()),
            4013 => Err(close.reason.to_string()),
            4014 => Err(close.reason.to_string()),
            _ => Ok(resume), //Err(format!("Unknown error code: {}", code)),
        },
        _ => Ok(resume), // Err(format!("Unknown error, code: {}", close.code)),
    }
}

pub struct DiscordEvLoop {
    heartbeat_interval: u64,
    sender: Option<Arc<UnboundedSender<String>>>,
    handles: Vec<JoinHandle<()>>,
}

impl DiscordEvLoop {
    pub fn new() -> Self {
        Self {
            heartbeat_interval: 41250,
            sender: None,
            handles: vec![],
        }
    }
}

impl DiscordEvLoop {
    pub async fn create_handles(
        &mut self,
        tx_res: Arc<UnboundedSender<Event>>,
        gateway_url: &str,
        resume: Option<&ResumeProperties>,
    ) -> Result<(), String> {
        let (mut ws_stream, _) = connect_async(format!("{gateway_url}/?v=10&encoding=json"))
            .await
            .map_err(|_| "failed to connect to the gateway")?;

        self.heartbeat_interval = get_heartbeat(&mut ws_stream)
            .await
            .map_err(|_| "failed to get heartbeat interval")?;

        if let Some(resume) = resume {
            let payload = serde_json::to_string(&GatewayResume {
                op: 6,
                d: ResumeProperties {
                    resume_gateway_url: String::from(&resume.resume_gateway_url),
                    session_id: String::from(&resume.session_id),
                    seq: resume.seq,
                    token: String::from(&resume.token),
                },
            })
            .map_err(|_| "failed to serialize resume event")?;

            ws_stream
                .send(Message::Text(payload))
                .await
                .map_err(|_| "failed to send resume event to the gateway")?;
        }

        let (tx, rx) = mpsc::unbounded_channel::<String>();
        let (write, read) = ws_stream.split();

        self.sender = Some(Arc::new(tx));

        self.handles = vec![
            self.recv(rx, write),
            self.handle_events(read, tx_res),
            self.heartbeat(),
        ];

        Ok(())
    }

    pub fn send(&self, payload: &str) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(sender) = &self.sender {
            sender.send(payload.to_string())?;

            return Ok(());
        }

        Ok(())
    }

    pub fn abort_tasks(&mut self) {
        for task in self.handles.iter() {
            task.abort();
        }

        self.handles.clear();
    }

    fn recv(&self, mut rx: UnboundedReceiver<String>, mut write: Write) -> JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(payload) = rx.recv().await {
                if let Err(err) = write.send(Message::Text(payload)).await {
                    println!("{err}")
                };
            }
        })
    }

    fn heartbeat(&self) -> JoinHandle<()> {
        let mut interval = time::interval(Duration::from_millis(self.heartbeat_interval));

        let tx = self.sender.as_ref().map(Arc::clone);

        tokio::spawn(async move {
            loop {
                interval.tick().await;

                if let Some(tx) = &tx {
                    if let Err(err) = tx.send(r#"{ "op": 1, "d": null }"#.to_string()) {
                        println!("{err}")
                    };
                }
            }
        })
    }

    fn handle_events(&self, mut read: Read, tx: Arc<UnboundedSender<Event>>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut broke = false;

            while let Some(Ok(ws_msg)) = read.next().await {
                match ws_msg {
                    Message::Text(msg) => {
                        let parsed_msg: Value = match serde_json::from_str(msg.as_str()) {
                            Ok(parsed) => parsed,
                            _ => continue,
                        };

                        if let Some(seq) = parsed_msg.get("s").and_then(|s| s.as_u64()) {
                            if let Err(err) = tx.send(Event::ResumeSeq(seq)) {
                                println!("{err}");
                            }
                        }

                        if let Some(op) = parsed_msg.get("op").and_then(|s| s.as_u64()) {
                            match op {
                                7 => {
                                    if let Err(err) = tx.send(Event::Resume) {
                                        println!("{err}")
                                    };

                                    broke = true;

                                    break;
                                }
                                9 => {
                                    println!("Invalid session");

                                    if let Some(true) =
                                        parsed_msg.get("d").and_then(|d| d.as_bool())
                                    {
                                        if let Err(err) = tx.send(Event::Resume) {
                                            println!("{err}")
                                        }
                                    }

                                    broke = true;

                                    break;
                                }
                                _ => (),
                            }
                        }

                        let event_name = match &parsed_msg.get("t").and_then(|s| s.as_str()) {
                            Some(event_name) => *event_name,
                            _ => continue,
                        };

                        if event_name == "READY" {
                            let resume_gateway_url = match &parsed_msg
                                .get("d")
                                .and_then(|d| d.get("resume_gateway_url"))
                                .and_then(|d| d.as_str())
                            {
                                Some(url) => String::from(*url),
                                _ => continue,
                            };
                            let session_id = match &parsed_msg
                                .get("d")
                                .and_then(|d| d.get("session_id"))
                                .and_then(|d| d.as_str())
                            {
                                Some(session_id) => String::from(*session_id),
                                _ => continue,
                            };

                            if let Err(err) =
                                tx.send(Event::ResumeProps((resume_gateway_url, session_id)))
                            {
                                println!("{err}")
                            };
                        }

                        let event = match create_event(event_name, &parsed_msg) {
                            Ok(event) => event,
                            _ => continue,
                        };

                        if let Err(err) = tx.send(event) {
                            println!("{err}");
                        };
                    }

                    Message::Close(close) => {
                        println!("Close: {close:?}");

                        match on_close(close) {
                            Ok(event) => {
                                if let Err(err) = tx.send(event) {
                                    println!("{err}");
                                }

                                broke = true;

                                break;
                            }
                            Err(err) => {
                                println!("{err}");

                                broke = true;

                                break;
                            }
                        };
                    }

                    _ => (),
                }
            }

            if !broke {
                if let Err(err) = tx.send(Event::Resume) {
                    println!("{err}")
                };
            }
        })
    }
}

#[derive(Debug)]
pub struct LavalinkEvLoop {
    pub sender: Option<Arc<UnboundedSender<String>>>,
    pub options: Rc<LavalinkBuilderOptions>,
    handles: Vec<JoinHandle<()>>,
}

impl LavalinkEvLoop {
    pub fn new(options: LavalinkBuilderOptions) -> Self {
        Self {
            sender: None,
            options: Rc::new(options),
            handles: vec![],
        }
    }

    pub async fn connect(&mut self, tx: Arc<UnboundedSender<Event>>) -> Result<(), String> {
        let (host, port) = (&self.options.host, self.options.port);

        let ws_uri = format!("ws://{host}:{port}/");

        let url = Request::builder()
            .method("GET")
            .uri(&ws_uri)
            .header("Host", &self.options.host)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_key())
            .header("Authorization", &self.options.password)
            .header(
                "User-Id",
                "1044312701637247087", /*self.options.user_id*/
            )
            .header("Client-Name", "franta-rust")
            .header("Resume-Key", "franta-rust-resume-key")
            .body(())
            .map_err(|_| "Failed to create request")?;

        let (ws_stream, _) = connect_async(url)
            .await
            .map_err(|_| "Failed to connect to the lavalink")?;

        let (mut write, mut read) = ws_stream.split();

        let (sender, rx) = mpsc::unbounded_channel::<String>();

        let sender = Arc::new(sender);

        self.sender = Some(Arc::clone(&sender));

        if let Err(err) = write
            .send(Message::Text(
                json!({
                    "op": "configureResuming",
                    "key": "franta-rust-resume-key",
                    "timeout": 60
                })
                .to_string(),
            ))
            .await
        {
            return Err(format!("Failed to send resume config: {err}"));
        };

        let handle = self.recv(rx, write);

        self.handles.push(handle);

        tokio::spawn(async move {
            while let Some(Ok(resp)) = read.next().await {
                match resp {
                    Message::Text(msg) => {
                        let parsed_msg: Value = match serde_json::from_str(&msg) {
                            Ok(parsed) => parsed,
                            _ => continue,
                        };

                        let guild_id = match parsed_msg.get("guildId").and_then(|id| id.as_str()) {
                            Some(guild_id) => guild_id,
                            _ => continue,
                        };

                        if let Some(event_type) = parsed_msg.get("type") {
                            match event_type.as_str() {
                                Some("TrackEndEvent") => {
                                    if let Err(err) =
                                        tx.send(Event::TrackEnd(String::from(guild_id)))
                                    {
                                        println!("lavalink: {err}");
                                    }
                                }
                                _ => continue,
                            }
                        };
                    }
                    Message::Close(close) => {
                        println!("Close: {close:?}");

                        if let Err(err) = tx.send(Event::LavalinkClosed) {
                            println!("lavalink: {err}");
                        }

                        break;
                    }
                    _ => (),
                }
            }

            println!("lavalink connection closed");
        });

        Ok(())
    }

    pub fn abort_tasks(&mut self) {
        for task in self.handles.iter() {
            task.abort();
        }

        self.handles.clear();
    }

    fn recv(&self, mut rx: UnboundedReceiver<String>, mut write: Write) -> JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(payload) = rx.recv().await {
                if let Err(err) = write.send(Message::Text(payload)).await {
                    println!("{err}")
                };
            }
        })
    }
}
