use std::time::Instant;

use log::info;

pub struct Timer {
        msg:   String,
        start: Instant,
}

impl Timer {
        pub fn new<S>(msg: S) -> Self
        where S: Into<String> {
                Timer {
                        msg:   msg.into(),
                        start: Instant::now(),
                }
        }
}

impl Drop for Timer {
        fn drop(&mut self) {
                let time = self.start.elapsed();

                info!("{}{:.3}ms", self.msg, time.as_micros() as f32 / 1000.0);
        }
}

macro_rules! timer {
        ($t:expr) => {
                let _t = Timer::new($t);
        };
}
