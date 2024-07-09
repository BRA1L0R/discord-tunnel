use std::{net::IpAddr, ops::DerefMut};

use anyhow::Context;
use base64::{prelude::BASE64_STANDARD, Engine};
use clap::Parser;
use teloxide_core::{
    requests::Requester,
    types::{ChatId, UpdateKind},
    Bot,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tun::{platform::Device, AsyncDevice, Configuration};

#[derive(Parser)]
struct Args {
    #[arg(short, long)]
    bot_token: String,

    #[arg(short, long)]
    read_group_id: i64,
    #[arg(short, long)]
    write_group_id: i64,

    #[arg(short, long)]
    address: IpAddr,
    #[arg(short, long)]
    destination_address: IpAddr,
}

struct BotAdapter {
    bot: Bot,

    read_group: ChatId,
    write_group: ChatId,

    offset: Option<i32>,
}

impl BotAdapter {
    fn new(bot: Bot, read_group: ChatId, write_group: ChatId) -> Self {
        Self {
            bot,
            read_group,
            write_group,
            offset: None,
        }
    }
}

impl BotAdapter {
    async fn read_packet(&mut self) -> anyhow::Result<Vec<u8>> {
        let message = loop {
            let mut updates = self.bot.get_updates();
            updates.offset = self.offset;
            let updates = updates.await?;

            self.offset = updates.last().map(|last| last.id + 1);

            let message = updates
                .into_iter()
                .filter_map(|update| {
                    return match update.kind {
                        UpdateKind::Message(message) if message.text().is_some() => Some(message),
                        _ => None,
                    };
                })
                .next();

            if let Some(message) = message {
                println!("{}", message.chat.id);
                break message;
            }
        };

        let text = message
            .text()
            .context("received a message that is not text")?;

        let decoded = BASE64_STANDARD.decode(text).context("decoding base64")?;
        Ok(decoded)
        // buffer.copy_from_slice(src)
    }

    async fn write_packet(&mut self, buffer: &[u8]) -> anyhow::Result<()> {
        let encoded = BASE64_STANDARD.encode(buffer);
        self.bot.send_message(self.group, encoded).await?;
        // user_filter: u32,
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let telegram = Bot::new(args.bot_token);

    // Configura
    let mut config = Configuration::default();
    config
        .address(args.address)
        .destination(args.destination_address)
        .netmask("255.255.255.0")
        .up();

    let device = Device::new(&config).unwrap();
    let mut device = AsyncDevice::new(device).unwrap();

    let mut adapter = BotAdapter::new(telegram.clone(), ChatId(args.group_id));

    tokio::spawn(async move {
        let mut buffer = Box::new([0; 1500]);

        loop {
            tokio::select! {
                packet = adapter.read_packet() => {
                    let packet = packet.context("error reading from adapter")?;
                    println!("A -> D: {packet:?}");
                    if let Err(err) = device.write(&packet).await {
                        println!("Error writing to device {err}");
                    };
                }
                read = device.read(buffer.deref_mut()) => {
                    let read = read.context("error reading from device")?;
                    println!("D -> A: {buffer:?}");
                    adapter.write_packet(&buffer[..read]).await.context("error writing to adapter")?;
                }
            }
        }
    })
    .await
    .unwrap()
}
