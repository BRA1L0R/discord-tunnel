//! old unused telegram driver that i hope will find a purpose in the future

struct BotAdapter {
    bot: Bot,

    read_group: ChatId,
    write_group: ChatId,

    write_sequence: u32,
    read_sequence: u32,
    // set_description: Some(String),
    // offset: Option<i32>,
}

impl BotAdapter {
    fn new(bot: Bot, read_group: ChatId, write_group: ChatId) -> Self {
        Self {
            bot,
            read_group,
            write_group,
            write_sequence: 0,
            read_sequence: 0,
            // offset: None,
        }
    }
}

impl PacketAdapter for BotAdapter {
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
