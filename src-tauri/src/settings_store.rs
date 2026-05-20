use std::path::PathBuf;

use crate::models::LauncherSettings;

#[cfg(target_os = "windows")]
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
#[cfg(target_os = "windows")]
use winreg::RegKey;

pub fn load_settings() -> LauncherSettings {
    let path = settings_path();
    let settings_file_exists = path.exists();
    let mut loaded = match std::fs::read_to_string(path) {
        Ok(value) => {
            serde_json::from_str::<LauncherSettings>(&value).unwrap_or_else(|_| default_settings())
        }
        Err(_) => default_settings(),
    };

    if let Ok(roots) = std::env::var("BUSCADOR_ROOTS") {
        let separator = if cfg!(target_os = "windows") {
            ';'
        } else {
            ':'
        };
        let parsed: Vec<String> = roots
            .split(separator)
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
        let autostart_enabled = is_windows_autostart_enabled();
        if autostart_enabled {
            loaded.start_with_windows = true;
        } else if !settings_file_exists {
            loaded.start_with_windows = true;
        }
    }

    #[cfg(target_os = "linux")]
    {
        let autostart_enabled = is_linux_autostart_enabled();
        if autostart_enabled {
            loaded.start_with_windows = true;
        } else if !settings_file_exists {
            loaded.start_with_windows = true;
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
                .join("fenix")
                .join("buscador.json");
        }
    }

    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        #[cfg(target_os = "windows")]
        return PathBuf::from(home)
            .join(".buscador-launcher")
            .join("settings.json");
        #[cfg(not(target_os = "windows"))]
        return PathBuf::from(home)
            .join(".config")
            .join("fenix")
            .join("buscador.json");
    }

    PathBuf::from("buscador-settings.json")
}

fn default_settings() -> LauncherSettings {
    LauncherSettings {
        start_with_windows: true,
        roots: vec![],
        max_files: 25_000,
        web_provider: String::new(),
        web_api_key: String::new(),
        theme: "system".to_string(),
    }
}

#[cfg(target_os = "windows")]
fn is_windows_autostart_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run_key = match hkcu
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", KEY_READ)
    {
        Ok(value) => value,
        Err(_) => return false,
    };

    match run_key.get_value::<String, _>("Buscador") {
        Ok(value) => !value.trim().is_empty(),
        Err(_) => false,
    }
}

#[cfg(target_os = "linux")]
fn is_linux_autostart_enabled() -> bool {
    linux_autostart_entry_path().exists() || linux_legacy_autostart_entry_path().exists()
}

#[cfg(target_os = "linux")]
fn linux_autostart_entry_path() -> PathBuf {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config_home)
            .join("autostart")
            .join("com.buscador.launcher.desktop");
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("autostart")
            .join("com.buscador.launcher.desktop");
    }

    PathBuf::from("com.buscador.launcher.desktop")
}

#[cfg(target_os = "linux")]
fn linux_legacy_autostart_entry_path() -> PathBuf {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config_home)
            .join("autostart")
            .join("buscador.desktop");
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("autostart")
            .join("buscador.desktop");
    }

    PathBuf::from("buscador.desktop")
}
