use std::path::Path;

use crate::{application::WindowMode, AnyResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ApplicationConfig {
        pub tps: u32,
        pub window_mode: WindowMode,
}

impl ApplicationConfig {
        pub fn from_file(path: &Path) -> AnyResult<Self> {
                let json = std::fs::read_to_string(path)?;

                Ok(serde_json::from_str(&json)?)
        }

        pub fn write(&self, path: &Path) -> AnyResult<()> {
                let json = serde_json::to_string_pretty(self)?;

                Ok(std::fs::write(path, json)?)
        }
}
