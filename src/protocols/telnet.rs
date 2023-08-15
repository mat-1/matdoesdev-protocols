use std::{io::Cursor, time::Duration};

use anyhow::bail;
use byteorder::{ReadBytesExt, BE};
use futures_util::StreamExt;
use tokio::{
    io::AsyncWriteExt,
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener,
    },
};
use tokio_util::codec::FramedRead;

use crate::{crawl::SiteData, terminal::TerminalSession};

use super::Protocol;

const BIND_HOST: &str = "[::]";
const BIND_PORT: u16 = {
    #[cfg(debug_assertions)]
    {
        2323
    }
    #[cfg(not(debug_assertions))]
    23
};

#[derive(Clone)]
pub struct Telnet {
    pub site_data: SiteData,
}

impl Protocol for Telnet {
    fn generate(data: &SiteData) -> Self {
        Telnet {
            site_data: data.clone(),
        }
    }

    async fn serve(self) {
        let listener = TcpListener::bind(format!("{BIND_HOST}:{BIND_PORT}"))
            .await
            .unwrap();

        loop {
            let (stream, _) = listener.accept().await.unwrap();
            println!("started tcp connection");

            let (read, write) = stream.into_split();

            let site_data = self.site_data.clone();
            tokio::spawn(async move {
                match connection(read, write, site_data).await {
                    Ok(_) => {}
                    Err(e) => {
                        println!("error: {}", e);
                    }
                }
            });
        }
    }
}

#[derive(Clone, Debug)]
enum Command {
    Subnegotiation(Subnegotiation),
    Will(Opt),
    Wont(Opt),
    Do(Opt),
    Dont(Opt),
}
#[derive(Clone, Copy, Debug)]
enum Opt {
    Echo = 1,
    SuppressGoAhead = 3,
    WindowSize = 31,
    LineMode = 34,
}
#[derive(Clone, Copy, Debug)]
enum Subnegotiation {
    WindowSize { width: u16, height: u16 },
}
impl Opt {
    fn from_u8(byte: u8) -> Option<Opt> {
        match byte {
            1 => Some(Opt::Echo),
            3 => Some(Opt::SuppressGoAhead),
            31 => Some(Opt::WindowSize),
            34 => Some(Opt::LineMode),
            _ => None,
        }
    }

    fn to_u8(&self) -> u8 {
        *self as u8
    }

    fn read(read: &mut Cursor<Vec<u8>>) -> anyhow::Result<Self> {
        let byte = read.read_u8()?;
        match Self::from_u8(byte) {
            Some(opt) => Ok(opt),
            None => bail!("unknown option {byte}"),
        }
    }
}

const IAC: u8 = 255;
const END_SUBNEGOTIATION: u8 = 240;

impl Command {
    fn read(read: &mut Cursor<Vec<u8>>) -> anyhow::Result<Self> {
        let byte = read.read_u8()?;
        match byte {
            250 => {
                let opt = Opt::read(read)?;
                match opt {
                    Opt::WindowSize => {
                        let width = read.read_u16::<BE>()?;
                        let height = read.read_u16::<BE>()?;
                        let _ = read.read_u8()?; // iac
                        let _ = read.read_u8()?; // end subnegotiation
                        Ok(Command::Subnegotiation(Subnegotiation::WindowSize {
                            width,
                            height,
                        }))
                    }
                    _ => bail!("unknown subnegotiation {opt:?}"),
                }
            }
            251 => Ok(Command::Will(Opt::read(read)?)),
            252 => Ok(Command::Wont(Opt::read(read)?)),
            253 => Ok(Command::Do(Opt::read(read)?)),
            254 => Ok(Command::Dont(Opt::read(read)?)),
            _ => bail!("unknown command {byte}"),
        }
    }

    async fn write(&self, write: &mut OwnedWriteHalf) -> anyhow::Result<()> {
        let mut buf = vec![IAC];
        match self {
            Command::Subnegotiation(subnegotiation) => {
                buf.push(250);
                match subnegotiation {
                    Subnegotiation::WindowSize { width, height } => {
                        buf.extend_from_slice(&[
                            31,
                            width.to_be_bytes()[0],
                            width.to_be_bytes()[1],
                        ]);
                        buf.extend_from_slice(&[height.to_be_bytes()[0], height.to_be_bytes()[1]]);
                    }
                }
                buf.extend_from_slice(&[IAC, END_SUBNEGOTIATION]);
            }
            Command::Will(opt) => buf.extend_from_slice(&[251, opt.to_u8()]),
            Command::Wont(opt) => buf.extend_from_slice(&[252, opt.to_u8()]),
            Command::Do(opt) => buf.extend_from_slice(&[253, opt.to_u8()]),
            Command::Dont(opt) => buf.extend_from_slice(&[254, opt.to_u8()]),
        };
        write.write_all(&buf).await?;
        Ok(())
    }
}

async fn connection(
    read: OwnedReadHalf,
    mut write: OwnedWriteHalf,
    site_data: SiteData,
) -> anyhow::Result<()> {
    let mut read = FramedRead::new(read, tokio_util::codec::BytesCodec::new());

    Command::Will(Opt::Echo).write(&mut write).await?;
    Command::Will(Opt::SuppressGoAhead)
        .write(&mut write)
        .await?;
    Command::Wont(Opt::LineMode).write(&mut write).await?;
    Command::Do(Opt::WindowSize).write(&mut write).await?;

    let mut terminal_session = TerminalSession::new(site_data);

    write.write_all(&terminal_session.on_open()).await?;

    loop {
        let Ok(read_result) = tokio::time::timeout(Duration::from_millis(100), read.next()).await
        else {
            // get window size every second
            Command::Do(Opt::WindowSize).write(&mut write).await?;
            continue;
        };
        let Some(data) = read_result.transpose()? else {
            break;
        };
        println!("{data:?}");
        let mut data = Cursor::new(data.to_vec());

        while data.remaining_slice().starts_with(&[IAC]) {
            let _ = data.read_u8()?;
            let command = match Command::read(&mut data) {
                Ok(command) => command,
                Err(err) => {
                    println!("{err}");
                    continue;
                }
            };
            match command {
                Command::Will(opt) => {
                    Command::Dont(opt).write(&mut write).await?;
                }
                Command::Wont(_) => {}
                Command::Do(_) => {}
                Command::Dont(_) => {}
                Command::Subnegotiation(subnegotiation) => match subnegotiation {
                    Subnegotiation::WindowSize { width, height } => {
                        write
                            .write_all(&terminal_session.resize(width as u32, height as u32))
                            .await?;
                    }
                },
            }
            continue;
        }
        let data = data.remaining_slice();
        let data = match data.strip_suffix(b"\0") {
            Some(data) => data,
            None => data,
        };
        if data == [3] || data == [4] {
            write.write_all(&terminal_session.on_close()).await?;
            write.write_all(b"Bye!\r\n").await?;
            break;
        }
        let out = terminal_session.on_keystroke(&data);
        write.write_all(&out).await?;
    }
    println!("connection closed");

    Ok(())
}
