use crate::constants::*;

use std::time::Duration;

#[derive(Debug)]
pub(crate) struct CallStats {
	pub size: usize,
	pub len: Duration,
}

// Start by just trying to find bandwidth.
// Then start worrying about IATs, talk burst length, silence lengths...
impl CallStats {
	pub fn new() -> Self {
		Self {
			size: 0,
			len: Default::default(),
		}
	}

	pub fn register_keepalive(&mut self) {
		self.size += KEEPALIVE_SIZE
			+ UDP_HEADER_LEN + IPV4_HEADER_LEN;
	}

	pub fn register_voice(&mut self, audio_size: usize) {
		self.size += audio_size
			+ UDP_HEADER_LEN + IPV4_HEADER_LEN
			+ CMAC_BYTES + RTP_BYTES;
	}

	pub fn sleep(&mut self, sleep_length: Duration) {
		self.len += sleep_length;
	}

	pub fn mbps(&self) -> f64 {
		let d = self.size as f64
			* 8.0 // bps
			/ (1024.0 * 1024.0); // mega

		let t = self.len.as_secs() as f64
			+ f64::from(self.len.subsec_millis()) / 1000.0;

		d / t
	}
}