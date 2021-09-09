use std::{error::Error, path::Path};

use crate::application::WindowMode;
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ApplicationConfig {
        pub tps: u32,
        pub window_mode: WindowMode,
}

impl ApplicationConfig {
        pub fn from_file(path: &Path) -> Result<Self, Box<dyn Error>>{
                let json = std::fs::read_to_string(path)?;

                Ok(serde_json::from_str(&json)?)
        }

        pub fn write(&self, path: &Path) -> Result<(), Box<dyn Error>> {
                let json = serde_json::to_string_pretty(self)?;

                Ok(std::fs::write(path, json)?)
        }
}