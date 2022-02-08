#[macro_use]
mod vec_map;
#[macro_use]
mod scoped_timer;
mod actions;
mod application;
mod application_config;
mod asset_manager;
pub mod camera;
mod constants;
mod euler_angles;
mod hashmap;
mod image;
mod input_manager;
mod logic_thread;
mod my_glm;
mod render_state_switcher;
mod renderer;
mod util;
mod vertex;
mod vk;

#[allow(unused_imports)]
#[macro_use]
extern crate const_cstr;
extern crate imgui;
extern crate nalgebra as na;
extern crate nalgebra_glm as glm;
extern crate vk_mem as vma;
#[macro_use]
extern crate enum_map;
extern crate approx;

use std::{error::Error, io::Write};

use application::Application;
use chrono::Local;
use env_logger::Env;
#[allow(unused_imports)]
use log::{info, trace, warn};

pub type AnyResult<T> = anyhow::Result<T>;

fn main() -> Result<(), Box<dyn Error>> {
        env_logger::Builder::from_env(Env::default().default_filter_or("trace"))
                .format(|buf, record| {
                        writeln!(
                                buf,
                                "[{} {:>5}]  {}",
                                Local::now().time().format("%H:%M:%S").to_string(),
                                record.level(),
                                record.args()
                        )
                })
                .init();

        let app = Application::new()?;

        app.run();
}
