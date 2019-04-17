#[macro_use]
extern crate log;

mod config;
mod trace;

pub use config::Config;

use byteorder::{
	LittleEndian,
	NetworkEndian,
	ReadBytesExt,
	WriteBytesExt,
};
use net2::{
	unix::UnixUdpBuilderExt,
	UdpBuilder,
};
use parking_lot::RwLock;
use rand::{
	distributions::{
		Uniform,
	},
	prelude::*,
};
use std::{
	collections::HashMap,
	io::{
		Read,
		Result as IoResult,
	},
	net::{
		UdpSocket,
		SocketAddr,
	},
	sync::Arc,
	thread,
	time::{
		Duration,
		Instant,
	},
};
use trace::*;

fn make_udp_socket(port: u16, non_block: bool) -> IoResult<UdpSocket> {
	let out = UdpBuilder::new_v4()?;
	
	out.reuse_address(true)?;

	if !cfg!(windows) {
		out.reuse_port(true)?;
	}

	//let out = out.bind(("127.0.0.1", port))?;
	let out = out.bind(("0.0.0.0", port))?;
	out.set_nonblocking(non_block)?;

	Ok(out)
}

pub fn client(config: &Config) {
	//let ts = trace::read_traces(&config.base_dir);
	let ts = trace::read_traces_memo(&config.base_dir);

	crossbeam::scope(|s| {
		for i in 0..config.thread_count {
			s.spawn(|_| {
				inner_client(&config, &ts);
			});
		}
	}).unwrap();
}

const CMAC_BYTES: usize = 16;
const RTP_BYTES: usize = 12;

fn inner_client(config: &Config, ts: &TraceHolder) {//&Vec<Trace>) {
	let mut rng = thread_rng();

	let draw = Uniform::new(0, ts.len());

	let port_distrib = Uniform::new(30000, 54000);
	let port = port_distrib.sample(&mut rng);

	let mut buf = [0u8; 1560];
	let mut rxbuf = [0u8; 1560];

	let ka_freq = Duration::from_secs(5);
	let mut ka_count: u64 = 1;
	let mut ka_buf = [0u8; 8];
	let mut ka_time = None;

	let start = Instant::now();
	let end = if config.randomise_duration {
		let time_distrib = Uniform::new(
			config.duration_lb,
			config.duration_ub
				.expect("No upper bound set: cannot randomise call time."),
		);
		Some(time_distrib.sample(&mut rng))
	} else {
		config.duration_ub
	};

	let port = 0;
	let socket = make_udp_socket(port, true).unwrap();

	//println!("Listening on port {:?}", port);
	let ssrc = rng.gen::<u32>();
	(&mut buf[8..12]).write_u32::<NetworkEndian>(ssrc);

	// IDEA: if we haven't passed the LB then draw another entry.
	// BUT if there's a RANDOM UB defined, we need to keep drawing to hit that.
	let mut not_gone = true;
	while not_gone || start.elapsed() < config.duration_lb || (config.randomise_duration && start.elapsed() < end.unwrap()) {
		not_gone = false;
		let el_lock = ts.get_trace(draw.sample(&mut rng));
		let el_guard = el_lock.read();

		let el = el_guard.as_ref()
			.expect("File should have been filled in if a read lock was fulfilled...");

		// FIXME: need to draw more sessions if we hit end prematurely...
		let mut last_size = None;
		for pkt in el {
			use PacketChainLink::*;
			let mut sleep_time = 20 + match pkt {
				Packet(p) => {
					let p = usize::from(p.get());
					last_size = Some(p);
					//println!("Sending packet of size {:?} (before CMAC, RTP, ... )", p);
					socket.send_to(&buf[..p + CMAC_BYTES + RTP_BYTES], &config.address);
					0
				},
				Missing(t) => {
					if let Some(p) = last_size {
						//println!("Sending packet of size {:?} (before CMAC, RTP, ... )", p);
						socket.send_to(&buf[..p + CMAC_BYTES + RTP_BYTES], &config.address);
					}
					0
				},
				Silence(t) => {
					//println!("Waiting for {:?}ms.", t);
					// Note: won't receive packets in here...
					let out = u64::from(*t);
					out.min(config.max_silence.unwrap_or(out))
				}
			};

			while sleep_time > 0 {
				thread::sleep(Duration::from_millis(sleep_time.min(20)));
				sleep_time -= 20;
			}

			// keep alive
			if ka_time.map(|t: Instant| t.elapsed() >= ka_freq).unwrap_or(true) {
				(&mut ka_buf[..]).write_u64::<LittleEndian>(ka_count);
				socket.send_to(&ka_buf[..], &config.address);
				ka_count += 1;
				ka_time = Some(Instant::now());
			}

			while let Ok((sz, addr)) = socket.recv_from(&mut rxbuf) {
				//println!("Received {:?} bytes from {:?}", sz, addr);
			}

			// may need to exit early
			if let Some(end) = end {
				if start.elapsed() >= end {
					return;
				}
			}
		}
	}
}

pub fn server(config: &Config) {
	// Okay, figure out what I want to do.
	// Simplification for now: cache IPs and ssrcs
	// (src picks randomly).
	// Just run it as one room, which everyone joins.
	// FIXME: assign SSRC and send out-of-band.
	let socket = make_udp_socket(config.port, false).unwrap();

	let mut rooms: Vec<Vec<SocketAddr>> = vec![vec![]];
	let mut ip_map: HashMap<u32, SocketAddr> = Default::default();
	let mut room_map: HashMap<SocketAddr, usize> = Default::default();
	let mut buf = [0u8; 1560];
	let mut rng = thread_rng();
	let room_size_distrib = Uniform::new(
		config.min_room_size,
		config.max_room_size,
	);
	let mut room_cap = room_size_distrib.sample(&mut rng);

	loop {
		if let Ok((sz, addr)) = socket.recv_from(&mut buf) {
			// println!("packet!");
			match classify(&buf[..sz]) {
				PacketType::KeepAlive => {
					// println!("ka!");
					// Bounce the message back to them.
					socket.send_to(&buf[..sz], addr);
				},
				PacketType::Rtp => {
					// println!("rtp!");
					// Find the room, send to everyone else in the room.
					let ssrc = (&buf[8..12]).read_u32::<NetworkEndian>().unwrap();

					let found_room = room_map.entry(addr)
						.or_insert_with(|| {
							rooms.last_mut()
								.and_then(|r| Some(r.push(addr)));
							rooms.len() - 1
						});

					// println!("host {:?} in room {:?}", addr, found_room);

					if let Some(room) = rooms.last() {
						if config.split_rooms && room.len() >= room_cap {
							room_cap = room_size_distrib.sample(&mut rng);
							rooms.push(vec![]);
						}
					}

					let _ = ip_map.insert(ssrc, addr);

					for o_addr in rooms[*found_room].iter() {
						if *o_addr != addr {
							socket.send_to(&buf[..sz], o_addr);
							//println!("Sent {} bytes to {:?}", sz, o_addr);
						}
					}
				}
			}
		}
	}
}

enum PacketType {
	KeepAlive,
	Rtp,
}

fn classify(bytes: &[u8]) -> PacketType {
	match bytes.len() {
		8 => PacketType::KeepAlive,
		_ => PacketType::Rtp,
	}
}
