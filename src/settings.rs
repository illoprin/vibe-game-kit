use std::{error::Error, fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::renderer::GraphicsSettings;

pub const SETTINGS_FILE: &str = "settings.toml";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Map to load on the next application launch.
    pub map_path: String,
    pub graphics: GraphicsSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            map_path: "assets/map1.map".into(),
            graphics: GraphicsSettings::default(),
        }
    }
}

impl AppSettings {
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(error) => {
                eprintln!("Could not read {}: {error}; using defaults", path.display());
                return Self::default();
            }
        };

        match toml::from_str::<Self>(&text) {
            Ok(mut settings) => {
                if settings.map_path.trim().is_empty() {
                    settings.map_path = Self::default().map_path;
                }
                settings
            }
            Err(error) => {
                eprintln!(
                    "Could not parse {}: {error}; using defaults",
                    path.display()
                );
                Self::default()
            }
        }
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), Box<dyn Error>> {
        let text = toml::to_string_pretty(self)?;
        fs::write(path, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::ShadowFilter;

    #[test]
    fn settings_round_trip_through_toml() {
        let mut settings = AppSettings::default();
        settings.map_path = "maps/night yard.map".into();
        settings.graphics.ssao_enabled = false;
        settings.graphics.ssao_radius = 3.25;
        settings.graphics.shadow_filter = ShadowFilter::Vsm;

        let toml = toml::to_string_pretty(&settings).expect("settings should serialize");
        let decoded: AppSettings = toml::from_str(&toml).expect("settings should deserialize");
        assert_eq!(decoded.map_path, settings.map_path);
        assert_eq!(decoded.graphics, settings.graphics);
    }

    #[test]
    fn missing_settings_use_defaults() {
        let decoded: AppSettings = toml::from_str("[graphics]\nssao_enabled = false\n")
            .expect("partial settings should deserialize");
        assert_eq!(decoded.map_path, "assets/map1.map");
        assert!(!decoded.graphics.ssao_enabled);
        assert_eq!(
            decoded.graphics.ssao_radius,
            GraphicsSettings::default().ssao_radius
        );
    }
}
