use std::path::PathBuf;

use crate::models::LauncherSettings;

#[cfg(target_os = "windows")]
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
#[cfg(target_os = "windows")]
use winreg::RegKey;

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

    if let Ok(provider) = std::env::var("BUSCADOR_WEB_PROVIDER") {
        loaded.web_provider = provider.trim().to_string();
    }

    if let Ok(api_key) = std::env::var("BUSCADOR_WEB_API_KEY") {
        loaded.web_api_key = api_key.trim().to_string();
    }

    #[cfg(target_os = "windows")]
    {
        loaded.start_with_windows = is_windows_autostart_enabled();
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
        start_with_windows: false,
        roots: vec![],
        max_files: 25_000,
        web_provider: String::new(),
        web_api_key: String::new(),
    }
}

#[cfg(target_os = "windows")]
fn is_windows_autostart_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run_key = match hkcu.open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        KEY_READ,
    ) {
        Ok(value) => value,
        Err(_) => return false,
    };

    match run_key.get_value::<String, _>("Buscador") {
        Ok(value) => !value.trim().is_empty(),
        Err(_) => false,
    }
}
