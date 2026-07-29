use serde::{Deserialize, Serialize};
use std::{
    env,
    fs,
    io,
    path::{Path, PathBuf},
};

const SETTINGS_LIMIT: u64 = 64 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Settings {
    pub server_url: String,
    pub topic: String,
    pub token: String,
    pub notify: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server_url: "https://ntfy.sh".to_owned(),
            topic: String::new(),
            token: String::new(),
            notify: true,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        Self::load_from(&settings_path()).unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to(&settings_path())
    }

    fn load_from(path: &Path) -> io::Result<Self> {
        let metadata = fs::metadata(path)?;
        if metadata.len() > SETTINGS_LIMIT {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "settings file is too large"));
        }
        let bytes = fs::read(path)?;
        serde_json::from_slice(&bytes).map_err(io::Error::other)
    }

    fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, bytes)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(temp, path)
    }
}

fn settings_path() -> PathBuf {
    let root = env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    root.join("NtfyWindowsClient").join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn settings_json_round_trip() {
        let value = Settings {
            server_url: "https://example.test".into(),
            topic: "alerts".into(),
            token: "secret".into(),
            notify: false,
        };
        let encoded = serde_json::to_vec(&value).unwrap();
        let decoded: Settings = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.server_url, value.server_url);
        assert_eq!(decoded.topic, value.topic);
        assert_eq!(decoded.token, value.token);
        assert_eq!(decoded.notify, value.notify);
    }
}
