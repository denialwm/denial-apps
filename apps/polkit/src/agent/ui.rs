//! Private, bounded JSON-lines IPC. No passwords in argv, environment, files or logs.
use super::Event;
use serde::Deserialize;
use serde_json::Value;
use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

/// The program that draws the dialog: this one, with `--prompt`.
#[derive(Clone)]
pub struct Config {
    pub program: PathBuf,
}

// Intentionally no Debug implementation: Response contains a secret.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Input {
    Hello { token: String },
    Select { uid: u32 },
    Response { prompt: u64, text: String },
    Cancel,
}
impl Drop for Input {
    fn drop(&mut self) {
        if let Self::Response { text, .. } = self {
            text.zeroize();
        }
    }
}

pub struct Ui {
    child: Child,
    outgoing: Sender<Value>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    _directory: tempfile::TempDir,
}

impl Ui {
    pub fn spawn(config: &Config, id: u64, events: Sender<Event>) -> Result<Self, String> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is required")?;
        let directory = tempfile::Builder::new()
            .prefix("denial-polkit-")
            .tempdir_in(runtime)
            .map_err(|e| e.to_string())?;
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        let path = directory.path().join("ui.sock");
        let listener = UnixListener::bind(&path).map_err(|e| e.to_string())?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let mut random = [0u8; 32];
        File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut random))
            .map_err(|e| e.to_string())?;
        let token: String = random.iter().map(|b| format!("{b:02x}")).collect();
        // A centered, undecorated overlay with exclusive keyboard focus; the
        // dialog draws its card inside it.
        let child = Command::new(&config.program)
            .arg("--prompt")
            .env("DENIAL_POLKIT_SOCKET", &path)
            .env("DENIAL_POLKIT_TOKEN", &token)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Could not start authentication UI: {e}"))?;
        let (outgoing, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            let _ = serve(listener, token, id, &events, receiver, &stopped);
            let _ = events.send(Event::UiClosed(id));
        });
        Ok(Self {
            child,
            outgoing,
            stop,
            thread: Some(thread),
            _directory: directory,
        })
    }

    pub fn send(&self, message: Value) {
        let _ = self.outgoing.send(message);
    }
    pub fn exited(&mut self) -> bool {
        self.child.try_wait().map_or(true, |v| v.is_some())
    }
}

impl Drop for Ui {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // This is only the agent-owned dialog process, never the compositor/session.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(
    listener: UnixListener,
    token: String,
    id: u64,
    events: &Sender<Event>,
    outgoing: Receiver<Value>,
    stop: &AtomicBool,
) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut stream = loop {
        if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Ok(());
        }
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Err(e) => return Err(e),
        }
    };
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    stream.set_write_timeout(Some(Duration::from_millis(100)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut buffer = Zeroizing::new(Vec::new());
    let mut hello = false;
    while !stop.load(Ordering::Acquire) {
        if !hello && Instant::now() >= deadline {
            return Ok(());
        }
        if hello {
            while let Ok(message) = outgoing.try_recv() {
                serde_json::to_writer(&mut stream, &message)?;
                stream.write_all(b"\n")?;
            }
        }
        match read_message(&mut reader, &mut buffer) {
            Ok(Some(input)) => {
                if !hello {
                    if matches!(&input, Input::Hello {token: supplied} if supplied == &token) {
                        hello = true;
                    } else {
                        return Err(io::Error::other("Invalid UI handshake"));
                    }
                } else if !matches!(input, Input::Hello { .. }) {
                    if events.send(Event::Ui(id, input)).is_err() {
                        return Ok(());
                    }
                } else {
                    return Err(io::Error::other("Repeated UI handshake"));
                }
            }
            Ok(None) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Preserve partial messages across read timeouts, and bound allocations before parsing.
fn read_message<R: BufRead>(
    reader: &mut R,
    buffer: &mut Zeroizing<Vec<u8>>,
) -> io::Result<Option<Input>> {
    let available = reader.fill_buf()?;
    if available.is_empty() {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    let count = available
        .iter()
        .position(|b| *b == b'\n')
        .map_or(available.len(), |i| i + 1);
    if buffer.len() + count > 16_384 {
        return Err(io::Error::other("UI message exceeds limit"));
    }
    buffer.extend_from_slice(&available[..count]);
    reader.consume(count);
    if buffer.last() != Some(&b'\n') {
        return Ok(None);
    }
    let input = serde_json::from_slice(buffer).map_err(|_| io::Error::other("Invalid UI message"));
    buffer.zeroize();
    buffer.clear();
    input.map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_secret_is_rejected_before_deserialization() {
        let mut reader = io::Cursor::new(vec![b'x'; 16_385]);
        assert!(read_message(&mut reader, &mut Zeroizing::new(Vec::new())).is_err());
    }
    #[test]
    fn fragmented_response_is_reassembled_and_buffer_wiped() {
        let mut buffer = Zeroizing::new(b"{\"type\":\"response\",\"prompt\":7,".to_vec());
        let mut reader = io::Cursor::new(b"\"text\":\"secret\"}\n");
        let result = read_message(&mut reader, &mut buffer).unwrap().unwrap();
        assert!(matches!(&result, Input::Response {prompt: 7, text} if text == "secret"));
        assert!(buffer.is_empty());
    }
}
