use pnet::datalink::NetworkInterface;
use std::{
	net::SocketAddr,
	time::Duration,
};

#[derive(Debug)]
pub struct Config {
	// Base directory
	pub base_dir: String,

	// Connectivity
	pub address: SocketAddr,
	pub port: u16,
	pub interface: Option<NetworkInterface>,
	pub ip_modifier: IpStrategy,

	// Call timing/control.
	pub max_silence: Option<u64>,
	pub duration_lb: Duration,
	pub duration_ub: Option<Duration>,
	pub randomise_duration: bool,
	pub constant: bool,
	pub refresh: bool,

	// Concurrent execution strains.
	pub thread_count: usize,

	// Server config stuff
	pub min_room_size: usize,
	pub max_room_size: usize,
	pub split_rooms: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum IpStrategy {
	Vanilla,
	Odd,
	Even,
}
