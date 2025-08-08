use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Auto-load episodes when navigating in podcasts/downloads
    pub auto_load_episodes: bool,
    /// Volume level (0.0 to 1.0)
    pub default_volume: f32,
    /// Skip interval in seconds
    pub skip_interval: u64,
    /// Theme preferences
    pub theme: ThemeSettings,
    /// Remote control settings
    pub remote_control: RemoteControlSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeSettings {
    pub accent_color: String,
    pub use_borders: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteControlSettings {
    pub enabled: bool,
    pub preferred_port: u16,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            auto_load_episodes: true, // Default enabled
            default_volume: 0.7,
            skip_interval: 15,
            theme: ThemeSettings::default(),
            remote_control: RemoteControlSettings::default(),
        }
    }
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            accent_color: "Green".to_string(),
            use_borders: true,
        }
    }
}

impl Default for RemoteControlSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            preferred_port: 8042,
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
        let settings = Self::load_from_file(&settings_path).unwrap_or_default();
        
        Ok(Self {
            settings_path,
            settings,
        })
    }

    pub fn get_settings_path() -> Result<PathBuf> {
        let home_dir = home::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
        
        let config_dir = home_dir.join(".config").join("pinepods-firewood");
        
        // Ensure the directory exists
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)?;
        }
        
        Ok(config_dir.join("settings.json"))
    }

    fn load_from_file(path: &PathBuf) -> Result<AppSettings> {
        if !path.exists() {
            return Ok(AppSettings::default());
        }
        
        let content = fs::read_to_string(path)?;
        let settings: AppSettings = serde_json::from_str(&content)?;
        Ok(settings)
    }

    pub fn save(&self) -> Result<()> {
        let content = serde_json::to_string_pretty(&self.settings)?;
        fs::write(&self.settings_path, content)?;
        log::info!("Settings saved to: {}", self.settings_path.display());
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

    pub fn remote_control_enabled(&self) -> bool {
        self.settings.remote_control.enabled
    }

    pub fn remote_control_port(&self) -> u16 {
        self.settings.remote_control.preferred_port
    }
}