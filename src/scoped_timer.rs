use std::time::Instant;

use log::info;

#[allow(dead_code)]
pub enum TimePrefix {
        Nano,
        Micro,
        Milli,
        Base,
}

pub struct ScopedTimer {
        msg: String,
        prefix: TimePrefix,
        begin: Instant,
}

impl ScopedTimer {
        pub fn new(msg: impl Into<String>, prefix: TimePrefix) -> Self {
                ScopedTimer {
                        msg: msg.into(),
                        prefix,
                        begin: Instant::now(),
                }
        }
}

impl Drop for ScopedTimer {
        fn drop(&mut self) {
                let time = self.begin.elapsed();

                match self.prefix {
                        TimePrefix::Nano => info!("{}{}ns", self.msg, time.as_nanos()),
                        TimePrefix::Micro => info!("{}{}µs", self.msg, time.as_micros()),
                        TimePrefix::Milli => info!("{}{}ms", self.msg, time.as_millis()),
                        TimePrefix::Base => info!("{}{:.1}s", self.msg, time.as_secs_f32()),
                };
        }
}

macro_rules! scoped_timer {
        ($t:expr, $p:expr) => {
                let _t = crate::scoped_timer::ScopedTimer::new($t, $p);
        };
}
