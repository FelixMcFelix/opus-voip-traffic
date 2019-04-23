use crate::constants::*;

use ewma::EWMA;
use std::{
	fmt::{
		Debug,
		Formatter,
		Result as FmtResult,
	},
	time::Duration,
};

#[derive(Debug)]
pub(crate) struct CallStats {
	pub size: usize,
	pub len: Duration,

	pub pkt_predictor: MyEwma,
	pub pkt_errs: Vec<f64>,
}

pub struct MyEwma {
	inner: EWMA,
}

impl MyEwma {
	pub fn new(alpha: f64) -> Self {
		Self {
			inner: EWMA::new(alpha),
		}
	}

	pub fn value(&self) -> f64 {
		self.inner.value()
	}

	pub fn add(&mut self, n: f64) -> f64 {
		self.inner.add(n)
	}
}

impl Debug for MyEwma {
	fn fmt(&self, f: &mut Formatter) -> FmtResult {
		write!(f, "EWMA(v:{:?})", self.inner.value())
	}
}

// Start by just trying to find bandwidth.
// Then start worrying about IATs, talk burst length, silence lengths...
impl CallStats {
	pub fn new() -> Self {
		Self {
			size: 0,
			len: Default::default(),

			// Remarkably, gives best results...
			pkt_predictor: MyEwma::new(0.5),
			pkt_errs: vec![],
		}
	}

	pub fn register_keepalive(&mut self) {
		self.size += KEEPALIVE_SIZE
			+ UDP_HEADER_LEN + IPV4_HEADER_LEN;
	}

	pub fn register_voice(&mut self, audio_size: usize, prevent_feedback: bool) {
		let sz = audio_size
			+ UDP_HEADER_LEN + IPV4_HEADER_LEN
			+ CMAC_BYTES + RTP_BYTES;
		let sz_f = sz as f64;

		self.size += sz;

		if !prevent_feedback && audio_size > 3 {
			self.pkt_errs.push(self.pkt_predictor.value() - sz_f);
			let _ = self.pkt_predictor.add(sz_f);
		}
	}

	pub fn sleep(&mut self, sleep_length: Duration) {
		self.len += sleep_length;
	}

	pub fn mbps(&self) -> f64 {
		let d = self.size as f64
			* 8.0 // bps
			/ (1024.0 * 1024.0); // mega

		let t = (self.len.as_secs() as f64)
			+ (f64::from(self.len.subsec_millis()) / 1000.0);

		d / t
	}

	pub fn predict_voice_pkt(&self) -> f64 {
		self.pkt_predictor.value()
	}

	pub fn predictor_rms(&self) -> f64 {
		let mut out = 0.0;

		for e in &self.pkt_errs {
			out += e * e;
		}

		(out / self.pkt_errs.len() as f64).sqrt()
	}
}