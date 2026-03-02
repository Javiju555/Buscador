use std::path::PathBuf;

use crate::models::LauncherSettings;

pub fn load_settings() -> LauncherSettings {
    let path = settings_path();
    let mut loaded = match std::fs::read_to_string(path) {
        Ok(value) => {
            serde_json::from_str::<LauncherSettings>(&value).unwrap_or_else(|_| default_settings())
        }
        Err(_) => default_settings(),
    };

    if let Ok(roots) = std::env::var("BUSCADOR_ROOTS") {
        let parsed: Vec<String> = roots
            .split(';')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        if !parsed.is_empty() {
            loaded.roots = parsed;
        }
    }

    if let Ok(max_files) = std::env::var("BUSCADOR_MAX_FILES") {
        if let Ok(parsed) = max_files.parse::<usize>() {
            loaded.max_files = parsed;
        }
    }

    loaded
}

pub fn save_settings(settings: &LauncherSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let text = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    std::fs::write(path, text).map_err(|error| error.to_string())
}

fn settings_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local_app_data)
                .join("BuscadorLauncher")
                .join("settings.json");
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config_home)
                .join("buscador-launcher")
                .join("settings.json");
        }
    }

    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        return PathBuf::from(home)
            .join(".buscador-launcher")
            .join("settings.json");
    }

    PathBuf::from("buscador-settings.json")
}

fn default_settings() -> LauncherSettings {
    LauncherSettings {
        roots: vec![],
        max_files: 25_000,
    }
}
