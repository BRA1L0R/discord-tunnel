mod discord;

use std::{net::IpAddr, ops::DerefMut};

use anyhow::Context;
use clap::Parser;

use discord::DiscordAdapter;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tun::{platform::Device, AsyncDevice, Configuration, Layer};

#[derive(Parser)]
struct Args {
    #[arg(short, long)]
    bot_token: String,
    #[arg(short, long)]
    channel_id: u64,

    #[arg(short, long)]
    address: IpAddr,
    #[arg(short, long)]
    destination_address: IpAddr,
}

trait PacketAdapter {
    async fn read_packet(&mut self, buffer: &mut [u8]) -> anyhow::Result<usize>;
    async fn write_packet(&mut self, packet: &[u8]) -> anyhow::Result<()>;
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Configura
    let mut config = Configuration::default();
    config
        .address(args.address)
        .destination(args.destination_address)
        .layer(Layer::L3)
        .netmask("255.255.255.0")
        .up();

    #[cfg(target_os = "linux")]
    config.platform(|config| {
        config.packet_information(false);
    });

    let device = Device::new(&config).unwrap();
    let mut tun_device = AsyncDevice::new(device).unwrap();

    let mut bot_adapter = DiscordAdapter::new(args.bot_token, args.channel_id.into()).await?;

    tokio::spawn(async move {
        let mut receive_buffer = Box::new([0; 2500]);
        let mut send_buffer = Box::new([0; 2500]);

        loop {
            tokio::select! {
                packet_size = bot_adapter.read_packet(send_buffer.deref_mut()) => {
                    let packet_size = packet_size.context("error reading from adapter")?;
                    if let Err(err) = tun_device.write(&send_buffer[..packet_size]).await {
                        println!("Error writing to device {err}");
                    };
                }
                read = tun_device.read(receive_buffer.deref_mut()) => {
                    let read = read.context("error reading from device")?;
                    bot_adapter.write_packet(&receive_buffer[..read]).await.context("error writing to adapter")?;
                }
            }
        }
    })
    .await
    .unwrap()
}
