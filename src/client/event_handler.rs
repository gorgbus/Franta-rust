// use super::{
//     events::{Event, ReadyUser},
//     LavalinkClient,
// };

// pub async fn event_handler(event: Event, manager: &LavalinkClient) {
//     match event {
//         Event::Ready(user) => on_ready(user),
//         _ => (),
//     }
// }

// fn on_ready(user: ReadyUser) {
//     println!("{}#{} has logged in!", user.username, user.discriminator);
// }

// async fn on_message_create(interaction: Interaction, client: &LavalinkClient) {
//     println!("{} said: {}", message.author.username, message.content);

//     if message.content == "!ping" {
//         message.channel().await.unwrap().send("pero").await.unwrap();
//     }
// }
