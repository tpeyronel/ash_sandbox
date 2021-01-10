use log::info;
use std::time::Instant;

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

		info!("{}{}ms ({}us)", self.msg, time.as_millis(), time.as_micros());
	}
}
