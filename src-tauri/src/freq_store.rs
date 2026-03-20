/// Persistent launch-frequency store for score boosting.
/// Stored at ~/.local/share/buscador/launch_counts.json
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub struct FreqStore {
    counts: HashMap<String, u32>,
    path: PathBuf,
}

impl FreqStore {
    pub fn load() -> Self {
        let path = freq_path();
        let counts = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { counts, path }
    }

    pub fn increment(&mut self, key: &str) {
        *self.counts.entry(key.to_string()).or_insert(0) += 1;
        self.save();
    }

    /// Logarithmic bonus: 1→43, 2→68, 5→115, 10→148, 20→181, 50→222
    pub fn score_bonus(&self, key: &str) -> i32 {
        let count = *self.counts.get(key).unwrap_or(&0);
        if count == 0 {
            return 0;
        }
        ((count as f32 + 1.0).ln() * 43.0) as i32
    }

    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(&self.counts) {
            let _ = fs::write(&self.path, json);
        }
    }
}

fn freq_path() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".local/share/buscador/launch_counts.json")
}
