use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Auto-load episodes when navigating in podcasts/downloads
    pub auto_load_episodes: bool,
    /// Volume level (0.0 to 1.0)
    pub default_volume: f32,
    /// Skip interval in seconds
    pub skip_interval: u64,
    /// Theme preferences
    pub theme: ThemeSettings,
    /// Audio settings
    pub audio: AudioSettings,
    /// UI settings
    pub ui: UiSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeSettings {
    pub theme_name: String,
    pub use_borders: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct AudioSettings {
    pub selected_device: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    /// Custom tab order - list of tab names in order
    pub tab_order: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            auto_load_episodes: true, // Default enabled
            default_volume: 0.7,
            skip_interval: 15,
            theme: ThemeSettings::default(),
            audio: AudioSettings::default(),
            ui: UiSettings::default(),
        }
    }
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            theme_name: "Light".to_string(),
            use_borders: true,
        }
    }
}


impl Default for UiSettings {
    fn default() -> Self {
        Self {
            tab_order: vec![
                "Home".to_string(),
                "Player".to_string(),
                "Feed".to_string(),
                "Podcasts".to_string(),
                "Queue".to_string(),
                "Saved".to_string(),
                "Downloads".to_string(),
                "Search".to_string(),
                "Settings".to_string(),
            ],
        }
    }
}

pub struct SettingsManager {
    settings_path: PathBuf,
    settings: AppSettings,
}

impl SettingsManager {
    pub fn new() -> Result<Self> {
        let settings_path = Self::get_settings_path()?;
        let settings = match Self::load_from_file(&settings_path) {
            Ok(settings) => settings,
            Err(e) => {
                tracing::warn!("couldn't read {}, using defaults: {e:#}", settings_path.display());
                AppSettings::default()
            }
        };
        Ok(Self {
            settings_path,
            settings,
        })
    }

    /// Defaults that won't be written anywhere, for when the config dir is unusable.
    pub fn with_defaults() -> Self {
        Self {
            settings_path: PathBuf::new(),
            settings: AppSettings::default(),
        }
    }

    pub fn get_settings_path() -> Result<PathBuf> {
        Ok(crate::paths::config_dir()?.join("settings.json"))
    }

    fn load_from_file(path: &PathBuf) -> Result<AppSettings> {
        if !path.exists() {
            return Ok(AppSettings::default());
        }
        let content = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn save(&self) -> Result<()> {
        if self.settings_path.as_os_str().is_empty() {
            return Ok(());
        }
        let content = serde_json::to_string_pretty(&self.settings)?;
        fs::write(&self.settings_path, content)?;
        tracing::debug!("settings saved to {}", self.settings_path.display());
        Ok(())
    }

    pub fn get(&self) -> &AppSettings {
        &self.settings
    }

    pub fn get_mut(&mut self) -> &mut AppSettings {
        &mut self.settings
    }

    pub fn update<F>(&mut self, updater: F) -> Result<()>
    where
        F: FnOnce(&mut AppSettings),
    {
        updater(&mut self.settings);
        self.save()
    }

    // Convenience getters
    pub fn auto_load_episodes(&self) -> bool {
        self.settings.auto_load_episodes
    }

    pub fn default_volume(&self) -> f32 {
        self.settings.default_volume
    }

    pub fn skip_interval(&self) -> u64 {
        self.settings.skip_interval
    }

    pub fn selected_audio_device(&self) -> Option<String> {
        self.settings.audio.selected_device.clone()
    }

    pub fn theme_name(&self) -> &str {
        &self.settings.theme.theme_name
    }

    pub fn tab_order(&self) -> &Vec<String> {
        &self.settings.ui.tab_order
    }

    pub fn set_tab_order(&mut self, tab_order: Vec<String>) -> Result<()> {
        self.settings.ui.tab_order = tab_order;
        self.save()
    }
}

/// Output devices as `(value, label)` pairs for the settings page, with the system
/// default first.
pub fn get_available_audio_devices() -> Vec<(String, String)> {
    let mut devices = vec![("default".to_string(), "System Default".to_string())];
    devices.extend(crate::audio::engine::output_device_names().into_iter().map(|name| {
        let label = crate::tui::text::truncate(&name, 40);
        (name, label)
    }));
    devices
}
