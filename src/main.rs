#[macro_use]
mod scoped_timer;
mod actions;
mod application;
mod application_config;
mod asset_manager;
pub mod camera;
mod components;
mod constants;
mod euler_angles;
mod hashmap;
mod image;
mod imgui_util;
mod input_manager;
mod model_instance_manager;
mod my_glm;
mod renderer;
mod shader_preprocessor;
mod shader_resource;
mod shader_resource_registry;
mod shader_resources;
mod skybox;
mod util;
mod vk;
mod window_manager;
mod winit_application;

#[allow(unused_imports)]
#[macro_use]
extern crate const_cstr;
extern crate imgui;
extern crate vk_mem as vma;
#[macro_use]
extern crate enum_map;
extern crate approx;

use std::{error::Error, io::Write};

use chrono::Local;
use env_logger::Env;
#[allow(unused_imports)]
use log::{info, trace, warn};
use winit_application::WinitApplication;

pub type AnyResult<T> = anyhow::Result<T>;

fn main() -> Result<(), Box<dyn Error>> {
        env_logger::Builder::from_env(Env::default().default_filter_or("trace"))
                .format(|buf, record| {
                        let style = buf.default_level_style(record.level());

                        let tag = format!("[{} {:>5}]", Local::now().time().format("%H:%M:%S"), record.level());

                        writeln!(buf, "{}  {}", style.value(tag), record.args())
                })
                .init();

        let app = WinitApplication::new();

        app.run()?;

        Ok(())
}
