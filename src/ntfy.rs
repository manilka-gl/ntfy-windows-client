use serde::Deserialize;
use std::{
    io::{self, BufRead, BufReader},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::SyncSender,
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

const MAX_STREAM_LINE_BYTES: usize = 1024 * 1024;
const MAX_DISPLAY_CHARS: usize = 16 * 1024;
const RECONNECT_MAX_SECONDS: u64 = 30;

#[derive(Clone, Debug)]
pub struct Connection {
    pub server: String,
    pub topic: String,
    pub token: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Message {
    pub id: String,
    pub topic: String,
    pub title: String,
    pub body: String,
    pub priority: i32,
    pub tags: Vec<String>,
    pub timestamp: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    Connected {
        generation: u64,
    },
    Status {
        generation: u64,
        message: String,
    },
    Message {
        generation: u64,
        message: Message,
    },
}

impl Event {
    #[must_use]
    pub const fn generation(&self) -> u64 {
        match self {
            Self::Connected { generation }
            | Self::Status { generation, .. }
            | Self::Message { generation, .. } => *generation,
        }
    }
}

#[derive(Debug, Deserialize)]
struct WireMessage {
    #[serde(default)]
    id: String,
    #[serde(default)]
    event: String,
    #[serde(default)]
    time: i64,
    #[serde(default)]
    topic: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    title: String,
    #[serde(default = "default_priority")]
    priority: i32,
    #[serde(default)]
    tags: Vec<String>,
}

const fn default_priority() -> i32 {
    3
}

pub fn spawn_subscription_worker(
    active_connection: Arc<Mutex<Option<Connection>>>,
    generation: Arc<AtomicU64>,
    sender: SyncSender<Event>,
) {
    thread::Builder::new()
        .name("ntfy-subscription".to_owned())
        .spawn(move || loop {
            let own_generation = generation.load(Ordering::Acquire);
            let connection = active_connection
                .lock()
                .ok()
                .and_then(|current| current.clone());
            let Some(connection) = connection else {
                thread::sleep(Duration::from_millis(200));
                continue;
            };
            if !run_subscription(connection, &generation, own_generation, &sender) {
                return;
            }
        })
        .expect("failed to start subscription thread");
}

pub fn publish(connection: &Connection, title: &str, body: &str) -> Result<(), String> {
    let agent = short_request_agent();
    let url = topic_url(connection);
    let mut request = agent
        .post(&url)
        .set(
            "User-Agent",
            concat!("ntfy-windows-client/", env!("CARGO_PKG_VERSION")),
        )
        .set("Content-Type", "text/plain; charset=utf-8");
    if !connection.token.is_empty() {
        request = request.set("Authorization", &format!("Bearer {}", connection.token));
    }
    if !title.trim().is_empty() {
        request = request.set("Title", title.trim());
    }
    request.send_string(body).map(|_| ()).map_err(error_text)
}

fn run_subscription(
    connection: Connection,
    generation: &AtomicU64,
    own_generation: u64,
    sender: &SyncSender<Event>,
) -> bool {
    let agent = stream_agent();
    let mut last_id = String::new();
    let mut delay = 1_u64;

    while generation.load(Ordering::Acquire) == own_generation {
        let url = stream_url(&connection, &last_id);
        let mut request = agent.get(&url).set(
            "User-Agent",
            concat!("ntfy-windows-client/", env!("CARGO_PKG_VERSION")),
        );
        if !connection.token.is_empty() {
            request = request.set("Authorization", &format!("Bearer {}", connection.token));
        }

        match request.call() {
            Ok(response) => {
                if generation.load(Ordering::Acquire) != own_generation {
                    return true;
                }
                if sender
                    .send(Event::Connected {
                        generation: own_generation,
                    })
                    .is_err()
                {
                    return false;
                }
                delay = 1;
                let mut reader = BufReader::with_capacity(8 * 1024, response.into_reader());
                let mut line = Vec::with_capacity(1024);
                loop {
                    if generation.load(Ordering::Acquire) != own_generation {
                        return true;
                    }
                    match read_line_limited(&mut reader, &mut line, MAX_STREAM_LINE_BYTES) {
                        Ok(0) => break,
                        Ok(_) => {
                            if generation.load(Ordering::Acquire) != own_generation {
                                return true;
                            }
                            if let Some(message) = parse_message(&line) {
                                last_id.clone_from(&message.id);
                                if sender
                                    .send(Event::Message {
                                        generation: own_generation,
                                        message,
                                    })
                                    .is_err()
                                {
                                    return false;
                                }
                            }
                        }
                        Err(error) => {
                            if sender
                                .send(Event::Status {
                                    generation: own_generation,
                                    message: format!("Stream error: {error}"),
                                })
                                .is_err()
                            {
                                return false;
                            }
                            break;
                        }
                    }
                }
            }
            Err(error) => {
                if generation.load(Ordering::Acquire) != own_generation {
                    return true;
                }
                if sender
                    .send(Event::Status {
                        generation: own_generation,
                        message: format!(
                            "Connection failed: {}. Retrying in {delay}s",
                            error_text(error)
                        ),
                    })
                    .is_err()
                {
                    return false;
                }
            }
        }

        if generation.load(Ordering::Acquire) != own_generation {
            return true;
        }
        thread::sleep(Duration::from_secs(delay));
        delay = (delay * 2).min(RECONNECT_MAX_SECONDS);
    }
    true
}

fn stream_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_write(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(70))
        .build()
}

fn short_request_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(20))
        .timeout_write(Duration::from_secs(20))
        .build()
}

fn topic_url(connection: &Connection) -> String {
    format!("{}/{}", connection.server, connection.topic)
}

fn stream_url(connection: &Connection, last_id: &str) -> String {
    let since = if last_id.is_empty() { "10m" } else { last_id };
    format!("{}/json?since={since}", topic_url(connection))
}

fn parse_message(line: &[u8]) -> Option<Message> {
    let wire: WireMessage = serde_json::from_slice(line).ok()?;
    if wire.event != "message" {
        return None;
    }
    Some(Message {
        id: wire.id,
        topic: wire.topic,
        title: truncate(&wire.title, MAX_DISPLAY_CHARS),
        body: truncate(&wire.message, MAX_DISPLAY_CHARS),
        priority: wire.priority.clamp(1, 5),
        tags: wire.tags.into_iter().take(32).collect(),
        timestamp: wire.time,
    })
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let result: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{result}…")
    } else {
        result
    }
}

fn error_text(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(code, response) => {
            let body = response.into_string().unwrap_or_default();
            let body = body.trim();
            if body.is_empty() {
                format!("HTTP {code}")
            } else {
                format!("HTTP {code}: {}", truncate(body, 512))
            }
        }
        ureq::Error::Transport(error) => error.to_string(),
    }
}

fn read_line_limited<R: BufRead>(
    reader: &mut R,
    output: &mut Vec<u8>,
    limit: usize,
) -> io::Result<usize> {
    output.clear();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(output.len());
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |position| position + 1);
        if output.len().saturating_add(take) > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "notification exceeds the 1 MiB safety limit",
            ));
        }
        output.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline.is_some() {
            return Ok(output.len());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_message, read_line_limited, Message};
    use std::io::{BufReader, Cursor};

    #[test]
    fn parses_message_event() {
        let value = br#"{"id":"abc","time":1700000000,"event":"message","topic":"alerts","message":"Disk full","title":"Server","priority":5,"tags":["warning"]}"#;
        assert_eq!(
            parse_message(value),
            Some(Message {
                id: "abc".to_owned(),
                topic: "alerts".to_owned(),
                title: "Server".to_owned(),
                body: "Disk full".to_owned(),
                priority: 5,
                tags: vec!["warning".to_owned()],
                timestamp: 1_700_000_000,
            })
        );
    }

    #[test]
    fn ignores_keepalive() {
        assert!(parse_message(br#"{"event":"keepalive"}"#).is_none());
    }

    #[test]
    fn rejects_oversized_line() {
        let input = vec![b'x'; 32];
        let mut reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();
        assert!(read_line_limited(&mut reader, &mut output, 16).is_err());
    }
}
