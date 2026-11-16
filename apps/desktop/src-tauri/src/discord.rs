//! Discord status ("Rich Presence"): the running game shows up in Discord.
//!
//! Talks to the local Discord client over its IPC socket (Unix socket
//! `discord-ipc-N` in the runtime folder, also Flatpak and Snap Discord; the
//! named pipe `\\.\pipe\discord-ipc-N` on Windows). Frames are a little-endian
//! opcode and length, then JSON. Nothing goes over the network from Cove, and
//! nothing happens unless the user switched it on (`general.discordStatus`).
//! A missing or closed Discord is never an error: the next status tries again.

use std::io::{Read, Write};
#[cfg(unix)]
use std::path::PathBuf;
use std::sync::mpsc;

use serde_json::{json, Value};

/// Cove's Discord application (its name is what Discord shows: "Cove").
/// Empty = this build has no Discord status.
pub const CLIENT_ID: &str = match option_env!("COVE_DISCORD_CLIENT_ID") {
    Some(id) => id,
    None => "",
};

const HANDSHAKE: u32 = 0;
const FRAME: u32 = 1;
/// Frames bigger than this are refused (Discord's answers are small).
const MAX_FRAME: u32 = 64 * 1024;

/// What Discord should show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Playing {
        title: String,
        /// An https cover image; Discord shows it next to the title.
        image: Option<String>,
        /// Unix seconds: Discord counts the time played from here.
        started_at: i64,
    },
    Clear,
}

pub fn available() -> bool {
    !CLIENT_ID.is_empty()
}

/// One frame: opcode, length, JSON.
pub fn encode(opcode: u32, payload: &Value) -> Vec<u8> {
    let body = payload.to_string().into_bytes();
    let mut frame = Vec::with_capacity(body.len() + 8);
    frame.extend_from_slice(&opcode.to_le_bytes());
    frame.extend_from_slice(&u32::try_from(body.len()).unwrap_or(u32::MAX).to_le_bytes());
    frame.extend_from_slice(&body);
    frame
}

fn read_frame(stream: &mut impl Read) -> std::io::Result<(u32, Value)> {
    let mut header = [0_u8; 8];
    stream.read_exact(&mut header)?;
    let opcode = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    if length > MAX_FRAME {
        return Err(std::io::Error::other("Discord frame too large"));
    }
    let mut body = vec![0_u8; length as usize];
    stream.read_exact(&mut body)?;
    Ok((opcode, serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

/// The `SET_ACTIVITY` command for a status.
pub fn activity(status: &Status, pid: u32, nonce: u64) -> Value {
    let activity = match status {
        Status::Clear => Value::Null,
        Status::Playing {
            title,
            image,
            started_at,
        } => {
            let mut activity = json!({
                "type": 0,
                "details": title,
                "timestamps": { "start": started_at },
            });
            if let Some(image) = image.as_deref().filter(|u| u.starts_with("https://")) {
                activity["assets"] = json!({ "large_image": image, "large_text": title });
            }
            activity
        }
    };
    json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": pid, "activity": activity },
        "nonce": nonce.to_string(),
    })
}

trait Pipe: Read + Write + Send {}
impl<T: Read + Write + Send> Pipe for T {}

/// Folders where Discord (native, Flatpak, Snap) puts its socket.
#[cfg(unix)]
fn socket_paths() -> Vec<PathBuf> {
    let mut bases: Vec<PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    bases.push(PathBuf::from("/tmp"));
    let mut paths = Vec::new();
    for base in bases {
        for sub in [
            "",
            "app/com.discordapp.Discord",
            "snap.discord",
            ".flatpak/com.discordapp.Discord/xdg-run",
        ] {
            for n in 0..10 {
                paths.push(base.join(sub).join(format!("discord-ipc-{n}")));
            }
        }
    }
    paths
}

#[cfg(unix)]
fn connect_pipe() -> Option<Box<dyn Pipe>> {
    socket_paths().into_iter().find_map(|path| {
        let stream = std::os::unix::net::UnixStream::connect(path).ok()?;
        let timeout = Some(std::time::Duration::from_secs(5));
        stream.set_read_timeout(timeout).ok()?;
        stream.set_write_timeout(timeout).ok()?;
        Some(Box::new(stream) as Box<dyn Pipe>)
    })
}

#[cfg(windows)]
fn connect_pipe() -> Option<Box<dyn Pipe>> {
    (0..10).find_map(|n| {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!(r"\\.\pipe\discord-ipc-{n}"))
            .ok()
            .map(|file| Box::new(file) as Box<dyn Pipe>)
    })
}

#[cfg(not(any(unix, windows)))]
fn connect_pipe() -> Option<Box<dyn Pipe>> {
    None
}

struct Connection {
    pipe: Box<dyn Pipe>,
    nonce: u64,
}

impl Connection {
    fn open() -> Option<Self> {
        let mut pipe = connect_pipe()?;
        pipe.write_all(&encode(
            HANDSHAKE,
            &json!({ "v": 1, "client_id": CLIENT_ID }),
        ))
        .ok()?;
        // READY, or an error (e.g. an unknown client id): then don't keep it.
        let (_, ready) = read_frame(&mut pipe).ok()?;
        (ready["evt"].as_str() == Some("READY")).then_some(Self { pipe, nonce: 0 })
    }

    fn send(&mut self, status: &Status) -> std::io::Result<()> {
        self.nonce += 1;
        let command = activity(status, std::process::id(), self.nonce);
        self.pipe.write_all(&encode(FRAME, &command))?;
        let (_, answer) = read_frame(&mut self.pipe)?;
        if answer["evt"].as_str() == Some("ERROR") {
            eprintln!("[discord] {}", answer["data"]["message"]);
        }
        Ok(())
    }
}

/// Sends statuses from a thread of its own (the socket blocks).
pub struct Discord {
    tx: Mutex<Option<mpsc::Sender<Status>>>,
}

use std::sync::Mutex;

impl Default for Discord {
    fn default() -> Self {
        Self {
            tx: Mutex::new(None),
        }
    }
}

impl Discord {
    pub fn set(&self, status: Status) {
        if !available() {
            return;
        }
        let mut tx = self
            .tx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sender = tx.get_or_insert_with(|| {
            let (sender, receiver) = mpsc::channel::<Status>();
            std::thread::spawn(move || worker(&receiver));
            sender
        });
        if sender.send(status).is_err() {
            *tx = None;
        }
    }
}

fn worker(receiver: &mpsc::Receiver<Status>) {
    let mut connection: Option<Connection> = None;
    while let Ok(mut status) = receiver.recv() {
        // Only the newest status matters.
        while let Ok(newer) = receiver.try_recv() {
            status = newer;
        }
        if connection.is_none() && status == Status::Clear {
            continue;
        }
        if connection.is_none() {
            connection = Connection::open();
        }
        if let Some(open) = connection.as_mut() {
            if open.send(&status).is_err() {
                // Discord was closed: try again with the next status.
                connection = None;
            }
        }
        if status == Status::Clear {
            // Closing the connection also removes the status.
            connection = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_opcode_length_and_json() {
        let frame = encode(FRAME, &json!({ "a": 1 }));
        assert_eq!(&frame[..4], &1_u32.to_le_bytes());
        assert_eq!(&frame[4..8], &7_u32.to_le_bytes());
        assert_eq!(&frame[8..], br#"{"a":1}"#);
        let (opcode, value) = read_frame(&mut frame.as_slice()).unwrap();
        assert_eq!((opcode, value), (1, json!({ "a": 1 })));

        let mut huge = 1_u32.to_le_bytes().to_vec();
        huge.extend_from_slice(&(MAX_FRAME + 1).to_le_bytes());
        assert!(read_frame(&mut huge.as_slice()).is_err());
    }

    #[test]
    fn the_activity_shows_title_time_and_cover() {
        let playing = Status::Playing {
            title: "Tidewater".into(),
            image: Some("https://images.igdb.com/cover.jpg".into()),
            started_at: 1_700_000_000,
        };
        let command = activity(&playing, 42, 7);
        assert_eq!(command["cmd"], "SET_ACTIVITY");
        assert_eq!(command["nonce"], "7");
        assert_eq!(command["args"]["pid"], 42);
        let shown = &command["args"]["activity"];
        assert_eq!(shown["details"], "Tidewater");
        assert_eq!(shown["timestamps"]["start"], 1_700_000_000);
        assert_eq!(
            shown["assets"]["large_image"],
            "https://images.igdb.com/cover.jpg"
        );

        // Local or plain-http images are left out.
        let local = Status::Playing {
            title: "Lantern".into(),
            image: Some("file:///home/u/cover.png".into()),
            started_at: 1,
        };
        assert!(activity(&local, 1, 1)["args"]["activity"]
            .get("assets")
            .is_none());
        assert_eq!(
            activity(&Status::Clear, 1, 1)["args"]["activity"],
            Value::Null
        );
    }

    #[cfg(unix)]
    #[test]
    fn talks_to_a_discord_socket() {
        use std::os::unix::net::UnixListener;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("discord-ipc-0");
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let (opcode, hello) = read_frame(&mut stream).unwrap();
            assert_eq!(opcode, HANDSHAKE);
            assert_eq!(hello["v"], 1);
            stream
                .write_all(&encode(
                    FRAME,
                    &json!({ "cmd": "DISPATCH", "evt": "READY" }),
                ))
                .unwrap();
            let (_, command) = read_frame(&mut stream).unwrap();
            stream
                .write_all(&encode(
                    FRAME,
                    &json!({ "cmd": "SET_ACTIVITY", "evt": null }),
                ))
                .unwrap();
            command
        });
        let stream = std::os::unix::net::UnixStream::connect(&path).unwrap();
        let mut pipe: Box<dyn Pipe> = Box::new(stream);
        pipe.write_all(&encode(HANDSHAKE, &json!({ "v": 1, "client_id": "1" })))
            .unwrap();
        let (_, ready) = read_frame(&mut pipe).unwrap();
        assert_eq!(ready["evt"], "READY");
        let mut connection = Connection { pipe, nonce: 0 };
        connection
            .send(&Status::Playing {
                title: "Tidewater".into(),
                image: None,
                started_at: 5,
            })
            .unwrap();
        let command = server.join().unwrap();
        assert_eq!(command["args"]["activity"]["details"], "Tidewater");
    }
}
