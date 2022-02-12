use std::time::Instant;

use log::info;

#[allow(dead_code)]
pub enum TimeUnit {
        Nanos,
        Micros,
        Millis,
        Secs,
}

pub struct ScopedTimer {
        msg: String,
        prefix: TimeUnit,
        begin: Instant,
}

impl ScopedTimer {
        pub fn new(msg: impl Into<String>, prefix: TimeUnit) -> Self {
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
                        TimeUnit::Nanos => info!("{}{}ns", self.msg, time.as_nanos()),
                        TimeUnit::Micros => info!("{}{}µs", self.msg, time.as_micros()),
                        TimeUnit::Millis => info!("{}{}ms", self.msg, time.as_millis()),
                        TimeUnit::Secs => info!("{}{:.1}s", self.msg, time.as_secs_f32()),
                };
        }
}

macro_rules! scoped_timer {
        ($t:expr) => {
                let _t = crate::scoped_timer::ScopedTimer::new($t, crate::scoped_timer::TimeUnit::Micros);
        };
        ($t:expr, Nanos) => {
                let _t = crate::scoped_timer::ScopedTimer::new($t, crate::scoped_timer::TimeUnit::Nanos);
        };
        ($t:expr, Micros) => {
                let _t = crate::scoped_timer::ScopedTimer::new($t, crate::scoped_timer::TimeUnit::Micros);
        };
        ($t:expr, Millis) => {
                let _t = crate::scoped_timer::ScopedTimer::new($t, crate::scoped_timer::TimeUnit::Millis);
        };
        ($t:expr, Secs) => {
                let _t = crate::scoped_timer::ScopedTimer::new($t, crate::scoped_timer::TimeUnit::Secs);
        };
}
