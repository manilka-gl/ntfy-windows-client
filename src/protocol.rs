use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct Message {
    pub id: String,
    pub time: i64,
    pub topic: String,
    pub title: String,
    pub body: String,
    pub priority: u8,
}

#[derive(Debug, Deserialize)]
struct WireEvent {
    #[serde(default)]
    id: String,
    #[serde(default)]
    time: i64,
    #[serde(default)]
    event: String,
    #[serde(default)]
    topic: String,
    #[serde(default)]
    title: String,
    #[serde(default, rename = "message")]
    body: String,
    #[serde(default = "default_priority")]
    priority: u8,
}

const fn default_priority() -> u8 {
    3
}

pub fn parse_line(line: &[u8]) -> Result<Option<Message>, serde_json::Error> {
    let event: WireEvent = serde_json::from_slice(line)?;
    if event.event != "message" || event.body.is_empty() {
        return Ok(None);
    }
    Ok(Some(Message {
        id: event.id,
        time: event.time,
        topic: event.topic,
        title: event.title,
        body: event.body,
        priority: event.priority.clamp(1, 5),
    }))
}

pub fn valid_topic(topic: &str) -> bool {
    let len = topic.len();
    (1..=64).contains(&len)
        && topic.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub fn sanitize_header(value: &str, max_chars: usize) -> String {
    value.chars().filter(|ch| !matches!(ch, '\r' | '\n' | '\0')).take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::{parse_line, sanitize_header, valid_topic};

    #[test]
    fn parses_message_event() {
        let message = parse_line(br#"{"id":"abc","time":1,"event":"message","topic":"x","title":"Hi","message":"Body","priority":5}"#)
            .unwrap()
            .unwrap();
        assert_eq!(message.id, "abc");
        assert_eq!(message.body, "Body");
        assert_eq!(message.priority, 5);
    }

    #[test]
    fn ignores_keepalive_event() {
        assert!(parse_line(br#"{"event":"keepalive"}"#).unwrap().is_none());
    }

    #[test]
    fn validates_topics() {
        assert!(valid_topic("build_alerts-1"));
        assert!(!valid_topic(""));
        assert!(!valid_topic("contains/slash"));
        assert!(!valid_topic("contains space"));
    }

    #[test]
    fn strips_header_injection() {
        assert_eq!(sanitize_header("hello\r\nX-Evil: yes", 100), "helloX-Evil: yes");
    }
}
