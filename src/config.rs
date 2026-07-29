use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{self, ErrorKind},
    path::PathBuf,
};

const CONFIG_FILE_LIMIT: u64 = 64 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Settings {
    pub server: String,
    pub topic: String,
    pub notifications_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server: "https://ntfy.sh".to_owned(),
            topic: String::new(),
            notifications_enabled: true,
        }
    }
}

impl Settings {
    pub fn load() -> io::Result<Self> {
        let path = config_path()?;
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        if metadata.len() > CONFIG_FILE_LIMIT {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "settings file is unexpectedly large",
            ));
        }
        let bytes = fs::read(path)?;
        serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
    }

    pub fn save(&self) -> io::Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
        fs::write(path, bytes)
    }
}

pub fn validate(server: &str, topic: &str) -> Result<(String, String), String> {
    let server = server.trim().trim_end_matches('/').to_owned();
    let topic = topic.trim().to_owned();

    if !(server.starts_with("https://") || server.starts_with("http://")) {
        return Err("Server must start with https:// or http://".to_owned());
    }
    if server.len() > 2048 {
        return Err("Server address is too long".to_owned());
    }
    if topic.is_empty() || topic.len() > 64 {
        return Err("Topic must contain 1 to 64 characters".to_owned());
    }
    if !topic
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("Topic may only contain letters, numbers, '-' and '_'".to_owned());
    }

    Ok((server, topic))
}

fn config_path() -> io::Result<PathBuf> {
    let base = env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(PathBuf::from))
        .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "configuration directory unavailable"))?;
    Ok(base.join("ntfy-windows-client").join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::validate;

    #[test]
    fn accepts_valid_connection() {
        let value = validate(" https://ntfy.sh/ ", "disk_alerts-1").unwrap();
        assert_eq!(value, ("https://ntfy.sh".to_owned(), "disk_alerts-1".to_owned()));
    }

    #[test]
    fn rejects_unsafe_topic_characters() {
        assert!(validate("https://ntfy.sh", "not/a/topic").is_err());
        assert!(validate("https://ntfy.sh", "topic name").is_err());
    }

    #[test]
    fn rejects_missing_scheme() {
        assert!(validate("ntfy.sh", "topic").is_err());
    }
}
