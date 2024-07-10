use std::{net::IpAddr, ops::DerefMut, time::Duration};

use anyhow::{bail, Context};
use base64::{prelude::BASE64_STANDARD, Engine};
use clap::Parser;
use teloxide_core::{
    requests::Requester,
    types::{ChatId, UpdateKind, UserId},
    Bot,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time,
};
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

    write_sequence: u32,
    read_sequence: u32,

    // set_description: Some(String),
    offset: Option<i32>,
}

impl BotAdapter {
    fn new(bot: Bot, read_group: ChatId, write_group: ChatId) -> Self {
        Self {
            bot,
            read_group,
            write_group,
            write_sequence: 0,
            read_sequence: 0,
            offset: None,
        }
    }
}

impl BotAdapter {
    async fn read_packet(&mut self) -> anyhow::Result<Vec<u8>> {
        loop {
            let mut set_description = self.bot.set_chat_description(self.read_group);

            set_description.description = Some(self.read_sequence.to_string());
            set_description.await.ok();
            let chat = self.bot.get_chat(self.read_group).await?;

            let title = chat.title().context("PORCOIDIOOO")?;

            println!("{title}");

            let mut decoded = base85::decode(title).context("decoding base64")?;
            let read_sequence: [u8; 4] = decoded.drain(decoded.len() - 4..).enumerate().fold(
                [0; 4],
                |mut collector, (i, n)| {
                    collector[i] = n;
                    collector
                },
            );

            let read_sequence: u32 = u32::from_be_bytes(read_sequence);
            if read_sequence <= self.read_sequence {
                time::sleep(Duration::from_millis(1000)).await;
                continue;
            }

            self.read_sequence = read_sequence;
            return Ok(decoded);
        }
    }

    async fn write_packet(&mut self, buffer: &[u8]) -> anyhow::Result<()> {
        // let encoded = BASE64_STANDARD.encode(buffer);

        let mut buffer = Vec::from(buffer);
        buffer.extend_from_slice(&self.write_sequence.to_be_bytes());
        self.write_sequence += 1;

        match buffer.len() {
            409.. => bail!("packet too big!!!"),
            204..=408 => {
                let mut set_chat_description = self.bot.set_chat_description(self.write_group);
                let description = &buffer[204..];
                let description = base85::encode(&description);
                set_chat_description.description = Some(description);

                set_chat_description.await?;
            }
            _ => (),
        }

        let title = &buffer[..std::cmp::min(buffer.len(), 204)];
        let title = base85::encode(title);
        self.bot.set_chat_title(self.write_group, title).await?;

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
    let mut tun_device = AsyncDevice::new(device).unwrap();

    let mut bot_adapter = BotAdapter::new(
        telegram.clone(),
        ChatId(args.read_group_id),
        ChatId(args.write_group_id),
    );

    tokio::spawn(async move {
        let mut buffer = Box::new([0; 1500]);

        loop {
            tokio::select! {
                packet = bot_adapter.read_packet() => {
                    let packet = packet.context("error reading from adapter")?;
                    println!("B -> T: {packet:?}");
                    if let Err(err) = tun_device.write(&packet).await {
                        println!("Error writing to device {err}");
                    };
                }
                read = tun_device.read(buffer.deref_mut()) => {
                    let read = read.context("error reading from device")?;
                    println!("T -> B: {:?}", &buffer[..read]);
                    bot_adapter.write_packet(&buffer[..read]).await.context("error writing to adapter")?;
                }
            }
        }
    })
    .await
    .unwrap()
}
