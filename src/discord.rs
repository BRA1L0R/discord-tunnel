use std::ops::DerefMut;
use std::sync::Arc;

use serenity::client::Context as SerenityContext;
use serenity::{
    all::{ChannelId, EventHandler, GatewayIntents, Message},
    Client,
};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::PacketAdapter;

pub struct DiscordAdapter {
    bot_http: Arc<serenity::http::Http>,
    channel: ChannelId,

    receiver_handle: JoinHandle<()>,
    message_receiver: mpsc::Receiver<Message>,

    encoding_buffer: Box<[u8; 2500]>,
}

impl DiscordAdapter {
    pub async fn new(token: impl AsRef<str>, channel: ChannelId) -> anyhow::Result<Self> {
        struct Handler {
            sender: mpsc::Sender<Message>,
        }

        #[serenity::async_trait]
        impl EventHandler for Handler {
            async fn message(&self, ctx: SerenityContext, message: Message) {
                if message.author.id == ctx.http.application_id().unwrap().get() {
                    return;
                }

                self.sender.send(message).await.unwrap();
            }
        }

        let (sender, receiver) = mpsc::channel(512);
        let handler = Handler { sender };

        let mut bot = Client::builder(
            token,
            GatewayIntents::MESSAGE_CONTENT | GatewayIntents::GUILD_MESSAGES,
        )
        .event_handler(handler)
        .await?;

        let http = bot.http.clone();
        let handle = tokio::spawn(async move {
            bot.start().await.unwrap();
        });

        Ok(Self {
            bot_http: http,
            channel,
            receiver_handle: handle,
            message_receiver: receiver,
            encoding_buffer: Box::new([0; 2500]),
        })
    }
}

impl Drop for DiscordAdapter {
    fn drop(&mut self) {
        self.receiver_handle.abort();
    }
}

fn collect_slice<T>(slice: &mut [T], iter: impl Iterator<Item = T>) -> usize {
    iter.zip(slice.iter_mut()).map(|(a, b)| *b = a).count()
}

fn collect_slice_try<T, E>(
    slice: &mut [T],
    iter: impl Iterator<Item = Result<T, E>>,
) -> Result<usize, E> {
    let mut count = 0;
    for (a, b) in iter.zip(slice.iter_mut()) {
        *b = a?;
        count += 1;
    }

    Ok(count)
}

impl PacketAdapter for DiscordAdapter {
    async fn read_packet(&mut self, buffer: &mut [u8]) -> anyhow::Result<usize> {
        let message = self.message_receiver.recv().await.unwrap();
        let content = message.content;

        let iter = base116::decode_str(&content);

        // fixes an incompatibility between how packets are passed to userspace
        // by the tun driver in macos and linux
        #[cfg(target_os = "macos")]
        let buffer = {
            (&mut buffer[..4]).copy_from_slice(&[0, 0, 0, 2]);
            &mut buffer[4..]
        };

        let count = collect_slice_try(buffer, iter)?;

        Ok(count + 4)
    }

    async fn write_packet(&mut self, packet: &[u8]) -> anyhow::Result<()> {
        #[cfg(target_os = "macos")]
        let packet = { &packet[4..] };

        let iter = base116::encode_to_bytes(packet.iter().copied());
        let count = collect_slice(self.encoding_buffer.deref_mut(), iter);

        let encoded = std::str::from_utf8(&self.encoding_buffer[..count]).unwrap();
        self.channel.say(&self.bot_http, encoded).await?;

        Ok(())
    }
}
