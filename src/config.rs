use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

const SETTINGS_LIMIT: u64 = 64 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct Settings {
    pub server_url: String,
    pub topic: String,
    pub notifications_enabled: bool,
    pub sound_enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_outputs: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub audio_output: String,
    pub placement: u8,
    pub auto_connect: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server_url: "https://ntfy.sh".to_owned(),
            topic: String::new(),
            notifications_enabled: true,
            sound_enabled: true,
            audio_outputs: Vec::new(),
            audio_output: String::new(),
            placement: 2,
            auto_connect: true,
        }
    }
}

impl Settings {
    #[must_use]
    pub fn load() -> Self {
        Self::load_from(&settings_path()).unwrap_or_default()
    }

    #[must_use]
    pub fn selected_audio_outputs(&self) -> Vec<String> {
        let selected = if self.audio_outputs.is_empty() {
            if self.audio_output.is_empty() {
                vec![String::new()]
            } else {
                vec![self.audio_output.clone()]
            }
        } else {
            self.audio_outputs.clone()
        };

        let mut unique = Vec::with_capacity(selected.len());
        for output in selected {
            if !unique.iter().any(|existing| existing == &output) {
                unique.push(output);
            }
        }
        if unique.is_empty() {
            unique.push(String::new());
        }
        unique
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to(&settings_path())
    }

    fn load_from(path: &Path) -> io::Result<Self> {
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        if metadata.len() > SETTINGS_LIMIT {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "settings file is unexpectedly large",
            ));
        }
        let bytes = fs::read(path)?;
        serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
    }

    fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec(self)
            .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, bytes)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(temporary, path)
    }
}

fn settings_path() -> PathBuf {
    let root = env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    root.join("ntfy-windows-client").join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn settings_json_round_trip() {
        let value = Settings {
            server_url: "https://example.test".into(),
            topic: "alerts".into(),
            notifications_enabled: false,
            sound_enabled: false,
            audio_outputs: vec!["Speakers".into(), "Headphones".into()],
            audio_output: String::new(),
            placement: 8,
            auto_connect: false,
        };
        let encoded = serde_json::to_vec(&value).unwrap();
        let decoded: Settings = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn legacy_single_audio_output_is_preserved() {
        let decoded: Settings = serde_json::from_str(
            r#"{"server_url":"https://ntfy.sh","topic":"alerts","notifications_enabled":true,"audio_output":"Headphones"}"#,
        )
        .unwrap();
        assert_eq!(decoded.selected_audio_outputs(), vec!["Headphones"]);
    }

    #[test]
    fn old_settings_receive_defaults() {
        let decoded: Settings = serde_json::from_str(
            r#"{"server_url":"https://ntfy.sh","topic":"alerts","notifications_enabled":true}"#,
        )
        .unwrap();
        assert!(decoded.sound_enabled);
        assert_eq!(decoded.selected_audio_outputs(), vec![""]);
        assert_eq!(decoded.placement, 2);
        assert!(decoded.auto_connect);
    }
}
