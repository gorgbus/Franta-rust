use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug)]
pub enum Event {
    Ready(ReadyUser),
    Resume,
    Reconnect,
    InteractionCreate(Interaction),
    VoiceStateUpdate(VoiceState),
    VoiceServerUpdate(VoiceServer),
    ResumeSeq(u64),
    ResumeProps((String, String)),
    SendWS(String),
    LavalinkClosed,
    TrackEnd(String),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReadyUser {
    pub avatar: Option<String>,
    pub bot: bool,
    pub discriminator: String,
    pub flags: u32,
    pub id: String,
    pub username: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DiscordMessage {
    pub channel_id: String,
    pub content: String,
    pub flags: u32,
    pub guild_id: String,
    pub id: String,
    pub member: Member,
    pub author: Author,
    token: Option<String>,
}

impl DiscordMessage {
    pub async fn channel(&self) -> Result<Channel, String> {
        let channel_id = &self.channel_id;

        let url = format!("https://discord.com/api/v10/channels/{channel_id}");

        let token = self.token.as_ref().ok_or("Token not found")?;

        let client = reqwest::Client::new();

        let mut res: Channel = client
            .get(url)
            // .bearer_auth(token)
            .header("Authorization", format!("Bot {token}"))
            .send()
            .await
            .map_err(|err| format!("Error fetching channel:\n\n{err}"))?
            .json()
            .await
            .map_err(|err| format!("Error parsing channel:\n\n{err}"))?;

        res.token = Some(String::from(token));

        Ok(res)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct VoiceState {
    pub channel_id: Option<String>,
    pub guild_id: String,
    pub user_id: String,
    pub session_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct VoiceServer {
    pub token: String,
    pub guild_id: String,
    pub endpoint: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Channel {
    pub id: String,
    pub name: String,
    token: Option<String>,
}

impl Channel {
    pub async fn send(&self, content: &str) -> Result<(), String> {
        let channel_id = &self.id;

        let url = format!("https://discord.com/api/v10/channels/{channel_id}/messages");

        let token = self.token.as_ref().ok_or("Token not found")?;

        let client = reqwest::Client::new();

        let body = json!({ "content": content });

        client
            .post(url)
            .header("Authorization", format!("Bot {token}"))
            .json(&body)
            .send()
            .await
            .map_err(|err| format!("Error sending message:\n\n{err}"))?;

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct User {
    pub avatar: Option<String>,
    pub discriminator: String,
    pub id: String,
    pub public_flags: u32,
    pub username: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Member {
    pub avatar: Option<String>,
    pub nick: Option<String>,
    pub roles: Vec<String>,
    pub user: User,
    #[serde(skip)]
    pub voice: Option<Arc<VoiceState>>,
}

impl Member {
    pub fn get_voice_channel(&self) -> &String {
        self.voice.as_ref().unwrap().channel_id.as_ref().unwrap()
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Author {
    pub avatar: Option<String>,
    pub discriminator: String,
    pub id: String,
    pub username: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Interaction {
    pub id: String,
    pub application_id: String,
    #[serde(rename = "type")]
    pub interaction_type: u32,
    pub data: Option<InteractionData>,
    pub guild_id: String,
    pub channel_id: String,
    pub member: Member,
    pub token: String,
}

fn rec_options<'i>(options: &'i Vec<InteractionDataOption>, name: &str) -> Option<&'i Value> {
    for option in options {
        if option.name.as_str() == name {
            return Some(option.value.as_ref().unwrap());
        }

        if let Some(opts) = option.options.as_ref() {
            match rec_options(opts, name) {
                Some(val) => return Some(val),
                None => continue,
            }
        }
    }

    None
}

impl Interaction {
    pub async fn ack(&self) -> Result<(), String> {
        let (id, token) = (&self.id, &self.token);

        let url = format!("https://discord.com/api/v10/interactions/{id}/{token}/callback");

        let client = reqwest::Client::new();

        let body = InteractionCallback {
            interaction_type: 5,
            data: None,
        };

        let res = client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|_| "Error sending ack")?;

        if res.status().is_success() {
            Ok(())
        } else {
            let status = res.status();

            Err(format!("Err: {status}, while trying to ack interaction"))
        }
    }

    pub async fn create_message(&self, data: InteractionCallbackData) -> Result<(), String> {
        let (id, token) = (&self.id, &self.token);

        let url = format!("https://discord.com/api/v10/interactions/{id}/{token}/callback");

        let client = reqwest::Client::new();

        let body = InteractionCallback {
            interaction_type: 4,
            data: Some(data),
        };

        let res = client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|_| "Error sending message")?;

        if res.status().is_success() {
            Ok(())
        } else {
            let status = res.status();

            Err(format!(
                "Err: {status}, while trying to respond to interaction"
            ))
        }
    }

    pub fn get_value(&self, name: &str) -> Option<&Value> {
        let data = self.data.as_ref()?;

        let options = data.options.as_ref()?;

        let value = rec_options(options, name)?;

        Some(value)
    }

    pub fn get_name(&self) -> Option<&str> {
        let data = self.data.as_ref()?;

        Some(data.name.as_str())
    }

    pub fn update_voice(mut self, voice: Arc<VoiceState>) -> Self {
        self.member.voice = Some(voice);
        self
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct InteractionData {
    pub id: String,
    pub name: String,
    pub options: Option<Vec<InteractionDataOption>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct InteractionDataOption {
    pub name: String,
    pub value: Option<Value>,
    pub options: Option<Vec<InteractionDataOption>>,
}

impl InteractionDataOption {
    pub fn get_name(&self) -> Option<&str> {
        Some(self.name.as_str())
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct InteractionCallback {
    #[serde(rename = "type")]
    interaction_type: u32,
    data: Option<InteractionCallbackData>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct InteractionCallbackData {
    pub tts: bool,
    pub content: String,
    pub embeds: Vec<Embed>,
    pub allowed_mentions: Option<AllowedMentions>,
    pub flags: Option<u32>,
}

impl InteractionCallbackData {
    pub fn new() -> Self {
        Self {
            tts: false,
            content: String::new(),
            embeds: Vec::new(),
            allowed_mentions: None,
            flags: None,
        }
    }

    pub fn set_content(mut self, content: String) -> Self {
        self.content = content;
        self
    }

    pub fn add_embed(mut self, embed: Embed) -> Self {
        self.embeds.push(embed);
        self
    }

    pub fn set_flags(mut self, flags: u32) -> Self {
        self.flags = Some(flags);
        self
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AllowedMentions {
    pub parse: Vec<String>,
    pub roles: Vec<String>,
    pub users: Vec<String>,
    pub replied_user: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Embed {
    pub title: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub timestamp: Option<String>,
    pub color: Option<u32>,
    pub footer: Option<EmbedFooter>,
    pub image: Option<EmbedImage>,
    pub thumbnail: Option<EmbedThumbnail>,
    pub video: Option<EmbedVideo>,
    pub author: Option<EmbedAuthor>,
    pub fields: Option<Vec<EmbedField>>,
}

impl Embed {
    pub fn new() -> Self {
        Self {
            title: None,
            description: None,
            url: None,
            timestamp: None,
            color: None,
            footer: None,
            image: None,
            thumbnail: None,
            video: None,
            author: None,
            fields: None,
        }
    }

    pub fn set_title(mut self, title: String) -> Self {
        self.title = Some(title);
        self
    }

    pub fn set_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    pub fn set_url(mut self, url: String) -> Self {
        self.url = Some(url);
        self
    }

    pub fn set_timestamp(mut self, timestamp: String) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    pub fn set_color(mut self, color: u32) -> Self {
        self.color = Some(color);
        self
    }

    pub fn set_footer(mut self, footer: EmbedFooter) -> Self {
        self.footer = Some(footer);
        self
    }

    pub fn set_image(mut self, image: EmbedImage) -> Self {
        self.image = Some(image);
        self
    }

    pub fn set_thumbnail(mut self, thumbnail: EmbedThumbnail) -> Self {
        self.thumbnail = Some(thumbnail);
        self
    }

    pub fn set_video(mut self, video: EmbedVideo) -> Self {
        self.video = Some(video);
        self
    }

    pub fn set_author(mut self, author: EmbedAuthor) -> Self {
        self.author = Some(author);
        self
    }

    pub fn add_field(mut self, field: EmbedField) -> Self {
        if let Some(fields) = self.fields.as_mut() {
            fields.push(field);
        } else {
            self.fields = Some(vec![field]);
        }

        self
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EmbedFooter {
    pub text: String,
    pub icon_url: Option<String>,
    pub proxy_icon_url: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EmbedImage {
    pub url: Option<String>,
    pub proxy_url: Option<String>,
    pub height: Option<u32>,
    pub width: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EmbedThumbnail {
    pub url: Option<String>,
    pub proxy_url: Option<String>,
    pub height: Option<u32>,
    pub width: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EmbedVideo {
    pub url: Option<String>,
    pub height: Option<u32>,
    pub width: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EmbedAuthor {
    pub name: Option<String>,
    pub url: Option<String>,
    pub icon_url: Option<String>,
    pub proxy_icon_url: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EmbedField {
    pub name: String,
    pub value: String,
    pub inline: bool,
}
