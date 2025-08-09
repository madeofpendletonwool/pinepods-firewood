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
    /// Audio settings
    pub audio: AudioSettings,
    /// UI settings
    pub ui: UiSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeSettings {
    pub theme_name: String,
    pub use_borders: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteControlSettings {
    pub enabled: bool,
    pub preferred_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    pub selected_device: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            remote_control: RemoteControlSettings::default(),
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

impl Default for RemoteControlSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            preferred_port: 8042,
        }
    }
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            selected_device: None, // None means use system default
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

// Audio device enumeration utilities
pub fn get_available_audio_devices() -> Vec<(String, String)> {
    use rodio::{cpal::traits::{DeviceTrait, HostTrait}};
    
    let mut devices = Vec::new();
    
    // Add default device option
    devices.push(("default".to_string(), "System Default".to_string()));
    
    // Redirect stderr to our log file during audio device enumeration
    // This way ALSA debug messages go to the log file instead of bleeding into TUI
    let log_file_path = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("pinepods")
        .join("logs")
        .join("pinepods.log");
    
    let _stderr_redirect = RedirectStderrToLog::new(&log_file_path);
    
    match std::panic::catch_unwind(|| {
        let host = rodio::cpal::default_host();
        let mut found_devices = Vec::new();
        
        // Log that we're enumerating audio devices
        log::debug!("Enumerating audio devices...");
        
        // Get output devices
        if let Ok(device_iter) = host.output_devices() {
            for device in device_iter {
                if let Ok(name) = device.name() {
                    let display_name = if name.len() > 40 {
                        format!("{}...", &name[..37])
                    } else {
                        name.clone()
                    };
                    found_devices.push((name.clone(), display_name));
                    log::debug!("Found audio device: {}", name);
                }
            }
        }
        
        log::debug!("Audio device enumeration completed, found {} devices", found_devices.len());
        found_devices
    }) {
        Ok(found_devices) => devices.extend(found_devices),
        Err(_) => {
            log::warn!("Audio device enumeration panicked, using default only");
        }
    }
    
    devices
}

// Helper to redirect stderr to log file during audio operations
struct RedirectStderrToLog {
    original_stderr: Option<std::fs::File>,
}

impl RedirectStderrToLog {
    fn new(log_file: &std::path::Path) -> Self {
        use std::os::unix::io::{AsRawFd, FromRawFd};
        
        unsafe {
            // Save original stderr
            let original_stderr_fd = libc::dup(2);
            let original_stderr = if original_stderr_fd >= 0 {
                Some(std::fs::File::from_raw_fd(original_stderr_fd))
            } else {
                None
            };
            
            // Open log file for appending
            if let Ok(log_file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_file) {
                // Redirect stderr to log file
                libc::dup2(log_file.as_raw_fd(), 2);
            }
            
            RedirectStderrToLog { original_stderr }
        }
    }
}

impl Drop for RedirectStderrToLog {
    fn drop(&mut self) {
        if let Some(original_stderr) = self.original_stderr.take() {
            use std::os::unix::io::AsRawFd;
            unsafe {
                // Restore original stderr
                libc::dup2(original_stderr.as_raw_fd(), 2);
            }
        }
    }
}