use serde::Deserialize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Message {
    pub id: String,
    pub time: i64,
    pub topic: String,
    pub title: String,
    pub body: String,
    pub priority: u8,
    pub tags: Vec<String>,
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
    #[serde(default)]
    tags: Vec<String>,
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
        topic: truncate_owned(event.topic, 256),
        title: truncate_owned(event.title, 256),
        body: truncate_owned(event.body, 16 * 1024),
        priority: event.priority.clamp(1, 5),
        tags: event
            .tags
            .into_iter()
            .take(16)
            .map(|tag| truncate_owned(tag, 64))
            .collect(),
    }))
}

pub fn valid_topic(topic: &str) -> bool {
    let len = topic.len();
    (1..=64).contains(&len)
        && topic
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub fn sanitize_header(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|character| !matches!(character, '\r' | '\n' | '\0'))
        .take(max_chars)
        .collect()
}

pub fn truncate(value: &str, max_chars: usize) -> String {
    if let Some((boundary, _)) = value.char_indices().nth(max_chars) {
        let mut output = String::with_capacity(boundary + '…'.len_utf8());
        output.push_str(&value[..boundary]);
        output.push('…');
        output
    } else {
        value.to_owned()
    }
}

pub fn truncate_owned(mut value: String, max_chars: usize) -> String {
    if let Some((boundary, _)) = value.char_indices().nth(max_chars) {
        value.truncate(boundary);
        value.push('…');
    }
    value
}

#[cfg(test)]
mod tests {
    use super::{Message, parse_line, sanitize_header, truncate, truncate_owned, valid_topic};

    #[test]
    fn parses_message_event() {
        let value = br#"{"id":"abc","time":1,"event":"message","topic":"x","title":"Hi","message":"Body","priority":5,"tags":["warning"]}"#;
        assert_eq!(
            parse_line(value).unwrap(),
            Some(Message {
                id: "abc".into(),
                time: 1,
                topic: "x".into(),
                title: "Hi".into(),
                body: "Body".into(),
                priority: 5,
                tags: vec!["warning".into()],
            })
        );
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
        assert_eq!(
            sanitize_header("hello\r\nX-Evil: yes", 100),
            "helloX-Evil: yes"
        );
    }

    #[test]
    fn truncates_at_character_boundary() {
        assert_eq!(truncate("abcdef", 3), "abc…");
        assert_eq!(truncate("åäö", 3), "åäö");
        assert_eq!(truncate_owned("abcdef".into(), 3), "abc…");
        assert_eq!(truncate_owned("åäö".into(), 3), "åäö");
    }
}
