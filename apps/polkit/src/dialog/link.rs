//! The dialog's end of the private socket to whoever started it, the agent
//! or denial-portal: UTF-8 JSON, one message per line, the token first.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;

use serde::Serialize;
use serde_json::Value;
use zeroize::Zeroizing;

/// The longest line read; the other end's own limit for what it sends.
const LONGEST: usize = 16_384;

pub struct Link {
    stream: UnixStream,
}

impl Link {
    /// Connects to the socket the variable `socket` names and says the token
    /// `token` names. The messages that arrive come out of the receiver,
    /// which closes when the other end does.
    pub fn connect(
        socket: &str,
        token: &str,
    ) -> io::Result<(Self, async_channel::Receiver<Value>)> {
        let missing = |name: &str| {
            io::Error::other(format!(
                "{name} is not set; start this dialog through its agent"
            ))
        };
        let path = std::env::var_os(socket).ok_or_else(|| missing(socket))?;
        let secret = Zeroizing::new(std::env::var(token).map_err(|_| missing(token))?);
        // Nothing this process starts needs them.
        // SAFETY: called before the dialog starts any other thread.
        unsafe {
            std::env::remove_var(socket);
            std::env::remove_var(token);
        }
        let stream = UnixStream::connect(path)?;
        let link = Self {
            stream: stream.try_clone()?,
        };
        link.write(&Hello {
            kind: "hello",
            token: &secret,
        })?;
        let (sender, receiver) = async_channel::unbounded();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stream);
            let mut line = Vec::new();
            loop {
                line.clear();
                match (&mut reader)
                    .take(LONGEST as u64 + 1)
                    .read_until(b'\n', &mut line)
                {
                    Ok(0) | Err(_) => break,
                    Ok(_) if line.len() > LONGEST => break,
                    Ok(_) => {}
                }
                let Ok(value) = serde_json::from_slice::<Value>(&line) else {
                    continue;
                };
                if sender.send_blocking(value).is_err() {
                    break;
                }
            }
        });
        Ok((link, receiver))
    }

    /// Sends one message.
    pub fn send(&self, message: &Value) -> io::Result<()> {
        self.write(message)
    }

    /// Sends a reply to the prompt `prompt`. The bytes this writes are
    /// cleared afterwards; the text field's own copies are not.
    pub fn respond(&self, prompt: u64, text: &str) -> io::Result<()> {
        self.write(&Response {
            kind: "response",
            prompt,
            text,
        })
    }

    fn write(&self, message: &impl Serialize) -> io::Result<()> {
        let mut bytes = Zeroizing::new(Vec::with_capacity(256));
        serde_json::to_writer(&mut *bytes, message)?;
        bytes.push(b'\n');
        (&self.stream).write_all(&bytes)
    }
}

#[derive(Serialize)]
struct Hello<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    token: &'a str,
}

#[derive(Serialize)]
struct Response<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    prompt: u64,
    text: &'a str,
}
