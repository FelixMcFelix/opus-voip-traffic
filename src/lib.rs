#[macro_use]
extern crate log;
#[macro_use]
extern crate speedy_derive;

mod config;
mod constants;
mod stats;
mod trace;

pub use config::*;
use constants::*;
use stats::CallStats;
use trace::*;

use byteorder::{
	LittleEndian,
	NetworkEndian,
	ReadBytesExt,
	WriteBytesExt,
};
use crossbeam::channel::{
	self,
	Receiver,
};
use net2::{
	unix::UnixUdpBuilderExt,
	UdpBuilder,
};
use pnet::{
	datalink::{
		self,
		Channel::Ethernet,
		DataLinkReceiver,
		DataLinkSender,
		MacAddr,
	},
	packet::{
		ethernet::{
			EtherTypes,
			MutableEthernetPacket,
		},
		ip::IpNextHeaderProtocols,
		ipv4::{
			self,
			MutableIpv4Packet,
		},
		udp::MutableUdpPacket,
	}
};
use rand::{
	distributions::{
		Uniform,
	},
	prelude::*,
};
use std::{
	collections::HashMap,
	io::{
		self,
		Result as IoResult,
	},
	net::{
		IpAddr,
		Ipv4Addr,
		UdpSocket,
		SocketAddr,
	},
	thread,
	time::{
		Duration,
		Instant,
	},
};

type RawSock = (Box<dyn DataLinkSender>, Box<dyn DataLinkReceiver>);

fn make_udp_socket(port: u16, non_block: bool) -> IoResult<UdpSocket> {
	let out = UdpBuilder::new_v4()?;
	
	out.reuse_address(true)?;

	if !cfg!(windows) {
		out.reuse_port(true)?;
	}

	let out = out.bind(("0.0.0.0", port))?;
	out.set_nonblocking(non_block)?;

	Ok(out)
}

pub fn convert_traces(config: &Config) {
	convert_all_traces(&config.base_dir);
}

pub fn client(config: &Config) {
	let ts = trace::read_traces_memo(&config.base_dir);
	let (kill_tx, kill_rx) = channel::bounded(1);

	crossbeam::scope(|s| {
		for _i in 0..config.thread_count {
			s.spawn(|_| {
				inner_client(&config, &ts, &kill_rx);
			});
		}

		if config.constant {
			let stdin = io::stdin();
			let mut s = String::new();

			stdin.read_line(&mut s);
			kill_tx.send(());
		}
	}).unwrap();
}

#[inline]
fn prep_packet(
	mut ethernet_buf: &mut [u8],
	src_port: u16,
	dest_addr: &SocketAddr,
	src_ip: Ipv4Addr,
	config: &Config,
) -> usize {
	if let Some(iface) = &config.interface {
		// sort of have to build from scratch if we want to write
		// straight over the NIC.
		{
			let mut eth_pkt = MutableEthernetPacket::new(&mut ethernet_buf)
				.expect("Plenty of room...");
			eth_pkt.set_destination(MacAddr::broadcast());
			// not important to set source, not interested in receiving a reply...
			eth_pkt.set_ethertype(EtherTypes::Ipv4);
			if let Some(mac) = &iface.mac {
				eth_pkt.set_source(*mac);
			}
		}

		{
			let mut ipv4_pkt = MutableIpv4Packet::new(&mut ethernet_buf[ETH_HEADER_LEN..])
				.expect("Plenty of room...");
			ipv4_pkt.set_version(4);
			ipv4_pkt.set_header_length(5);
			ipv4_pkt.set_ttl(64);
			ipv4_pkt.set_next_level_protocol(IpNextHeaderProtocols::Udp);
			ipv4_pkt.set_destination(match dest_addr.ip() {
				IpAddr::V4(a) => a,
				_ => panic!("IPv6 currently unsupported."),
			});
			ipv4_pkt.set_source(src_ip);
		}

		{
			let mut udp_pkt = MutableUdpPacket::new(&mut ethernet_buf[ETH_HEADER_LEN + IPV4_HEADER_LEN..])
				.expect("Plenty of room...");
			udp_pkt.set_source(src_port);
			udp_pkt.set_destination(dest_addr.port());
			// checksum is optional in ipv4
			udp_pkt.set_checksum(0);
		}

		ETH_HEADER_LEN + IPV4_HEADER_LEN + UDP_HEADER_LEN
	} else {
		0
	}
}

#[inline]
fn send_packet(
	ethernet_buf: &mut [u8],
	physical_if: &mut Option<RawSock>,
	dest_addr: &SocketAddr,
	udp_sock: &UdpSocket,
	udp_len: usize,
) {
	if let Some((tx, _rx)) = physical_if {
		{
			let mut ipv4_pkt = MutableIpv4Packet::new(&mut ethernet_buf[ETH_HEADER_LEN..])
				.expect("Plenty of room...");
			ipv4_pkt.set_total_length((IPV4_HEADER_LEN + UDP_HEADER_LEN + udp_len) as u16);

			let csum = ipv4::checksum(&ipv4_pkt.to_immutable());
			ipv4_pkt.set_checksum(csum);
		}

		{
			let mut udp_pkt = MutableUdpPacket::new(&mut ethernet_buf[ETH_HEADER_LEN + IPV4_HEADER_LEN..])
				.expect("Plenty of room...");
			udp_pkt.set_length((UDP_HEADER_LEN + udp_len) as u16);
		}

		tx.send_to(&ethernet_buf, None);
	} else {
		let _ = udp_sock.send_to(&ethernet_buf[..udp_len], dest_addr);
	}
}

fn inner_client(config: &Config, ts: &TraceHolder, kill_signal: &Receiver<()>) {
	let mut rng = thread_rng();

	let mut naked_if = config.interface.as_ref()
		.map(|iface| match datalink::channel(&iface, Default::default()) {
			Ok(Ethernet(tx, rx)) => (tx, rx),
			Ok(_) => panic!("Unexpected channel variant."),
			Err(e) => panic!("Failed to bind to interface (MAY NEED SUDO): {:?}", e),
		});

	let ephemeral_ports = Uniform::new_inclusive(31000, 61000);
	let draw = Uniform::new(0, ts.len());
	let ip_draw = Uniform::new(0, config.addresses.len());

	let mut buf = [0u8; 1560];
	let mut rxbuf = [0u8; 1560];

	let ka_freq = Duration::from_millis(KEEPALIVE_FREQ_MS);
	let mut ka_count: u64 = 1;
	let mut ka_buf = [0u8; ETH_HEADER_LEN + IPV4_HEADER_LEN + UDP_HEADER_LEN + KEEPALIVE_SIZE];
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

	let socket = make_udp_socket(0, true).unwrap();
	let mut desired_ip = generate_ip4(&mut rng, config.ip_modifier);
	let mut port = rng.sample(ephemeral_ports);
	let mut dest_addr = config.addresses.get(rng.sample(ip_draw))
		.expect("Should really be a valid selection...");

	info!("Drew target IP {:?}", dest_addr);

	let mut space_start = prep_packet(
		&mut buf,
		port,
		dest_addr,
		desired_ip,
		config,
	);
	let _ = prep_packet(
		&mut ka_buf,
		port,
		dest_addr,
		desired_ip,
		config,
	);

	let mut ssrc = rng.gen::<u32>();
	(&mut buf[space_start+SSRC_START..space_start+SSRC_END]).write_u32::<NetworkEndian>(ssrc)
		.expect("Guaranteed to be large enough.");

	// IDEA: if we haven't passed the LB then draw another entry.
	// BUT if there's a RANDOM UB defined, we need to keep drawing to hit that.
	let mut not_gone = true;
	while not_gone || config.constant || start.elapsed() < config.duration_lb || (config.randomise_duration && start.elapsed() < end.unwrap()) {
		// generate (another) identity.
		if config.refresh {
			desired_ip = generate_ip4(&mut rng, config.ip_modifier);

			port = rng.sample(ephemeral_ports);

			space_start = prep_packet(
				&mut buf,
				port,
				dest_addr,
				desired_ip,
				config,
			);

			let _ = prep_packet(
				&mut ka_buf,
				port,
				dest_addr,
				desired_ip,
				config,
			);

			ssrc = rng.gen::<u32>();
			(&mut buf[space_start+SSRC_START..space_start+SSRC_END]).write_u32::<NetworkEndian>(ssrc)
				.expect("Guaranteed to be large enough.");

			ka_count = 1;
			ka_time = None;

			dest_addr = config.addresses.get(rng.sample(ip_draw))
				.expect("Should really be a valid selection...");
		}

		not_gone = false;
		let chosen_el = draw.sample(&mut rng);
		info!("Chose trace no: {}", chosen_el);

		let el_lock = ts.get_trace(chosen_el);
		let el_guard = el_lock.read();

		let el = el_guard.as_ref()
			.expect("File should have been filled in if a read lock was fulfilled...");

		// FIXME: need to draw more sessions if we hit end prematurely...
		let mut stat = CallStats::new();

		for pkt in el {
			// FIXME: make sleep routine aware of any excess time consumed in packet prep/send.
			let (mut sleep_time, pkt_size) = handle_link(
				*pkt,
				&mut stat,
				&config.max_silence,
			);

			if pkt_size > 0 {
				let udp_payload_size = pkt_size + CMAC_BYTES + RTP_BYTES;

				// Fill all but the SSRC with random data.
				let (_other_proto, udp_body) = buf.split_at_mut(space_start);
				rng.fill(&mut udp_body[..SSRC_START]);
				rng.fill(&mut udp_body[SSRC_END..udp_payload_size]);

				info!("Sending packet of size {} ({} audio).", udp_payload_size, pkt_size);
				send_packet(
					&mut buf[..space_start+udp_payload_size],
					&mut naked_if,
					dest_addr,
					&socket,
					udp_payload_size,
				);
			}

			loop {
				// may need to exit early
				if let Some(end) = end {
					if start.elapsed() >= end {
						return;
					}
				}

				if kill_signal.is_full() {
					return;
				}

				// send keep alive if needed.
				if ka_time.map(|t: Instant| t.elapsed() >= ka_freq).unwrap_or(true) {
					(&mut ka_buf[space_start..space_start+KEEPALIVE_SIZE]).write_u64::<LittleEndian>(ka_count)
						.expect("Guaranteed to be large enough.");

					info!("Sending keep-alive {}.", ka_count);
					send_packet(
						&mut ka_buf[..space_start+KEEPALIVE_SIZE],
						&mut naked_if,
						dest_addr,
						&socket,
						KEEPALIVE_SIZE,
					);
					ka_count += 1;
					ka_time = Some(Instant::now());
				}

				while let Ok((sz, addr)) = socket.recv_from(&mut rxbuf) {
					info!("Received {:?} bytes from {:?}", sz, addr);
				}

				if sleep_time == 0 {
					break;
				}

				thread::sleep(Duration::from_millis(sleep_time.min(20)));
				sleep_time = sleep_time.checked_sub(20).unwrap_or(0);
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
			match classify(&buf[..sz]) {
				PacketType::KeepAlive => {
					// Bounce the message back to them.
					let _ = socket.send_to(&buf[..sz], addr);
					info!("Saw keepalive, replying...");
				},
				PacketType::Rtp => {
					// Find the room, send to everyone else in the room.
					let ssrc = (&buf[SSRC_START..SSRC_END]).read_u32::<NetworkEndian>().unwrap();

					let found_room = room_map.entry(addr)
						.or_insert_with(|| {
							rooms.last_mut()
								.and_then(|r| {
									r.push(addr);
									Some(())
								});
							rooms.len() - 1
						});

					if let Some(room) = rooms.last() {
						if config.split_rooms && room.len() >= room_cap {
							room_cap = room_size_distrib.sample(&mut rng);
							rooms.push(vec![]);
						}
					}

					let _ = ip_map.insert(ssrc, addr);

					let room = &rooms[*found_room];
					info!("Saw message from {:?}, forwarding ({})...", addr, room.len());
					info!("Total rooms {:?}...", rooms.len());

					for o_addr in room {
						if *o_addr != addr {
							let _ = socket.send_to(&buf[..sz], o_addr);
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

pub fn gen_stats(config: &Config) {
	let ts = trace::read_traces_memo(&config.base_dir);
	let mut t_err = 0.0;

	let mut kbps_holders = vec![vec![]];
	if config.max_silence.is_some() {
		kbps_holders.push(vec![]);
	}

	// FIXME: not playing nicely
	// crossbeam::scope(|s| {
		for i in 0..ts.len() {
			// |j| s.spawn(|_| {
				let (kbps, d) = inner_stats(&config, &ts, i);
				if d.is_finite() {
					t_err += d;
				}

				for (j, measure) in kbps.iter().enumerate() {
					if measure.is_finite() {
						kbps_holders[j].push(*measure);
					}
				}

			// });
		}
	// }).unwrap();

	println!("Missing Pkt Prediction Error: {:?}", t_err);

	for (i, holder) in kbps_holders.iter_mut().enumerate() {
		holder.sort_by(|a, b| a.partial_cmp(b).unwrap());
		let length = holder.len();

		let index = length / 2;
		let median = if length % 2 == 0 {
			(holder[index] + holder[index+1]) / 2.0
		} else {
			holder[index]
		};

		let sum: f64 = holder.iter().sum();

		let mean: f64 = sum / length as f64;

		println!("[{}] Mean: {} ({}), Median: {} ({})",
			if i == 0 {"True"} else {"MaxSilence"},
			mean, mean / 96.0,
			median, median / 96.0,
		);
		// println!("{:?}", holder);
	}
}

// Could refactor this to reduce code dup, but I think
// that might be complex (dummy senders etc...).
fn inner_stats(config: &Config, ts: &TraceHolder, i: usize) -> (Vec<f64>, f64) {
	// Will be easiest to maintain a callstat for each,
	// since keepalives will make things trickier undoubtedly...
	let mut stats = vec![(CallStats::new(), None)];

	let el_lock = ts.get_trace(i);
	let el_guard = el_lock.read();

	let el = el_guard.as_ref()
		.expect("File should have been filled in if a read lock was fulfilled...");

	if config.max_silence.is_some() {
		stats.push((CallStats::new(), config.max_silence));
	}

	for (ref mut stat, ref max_silence) in &mut stats {
		let mut ka_time = KEEPALIVE_FREQ_MS;

		for pkt in el {
			let (mut sleep_time, _pkt_size) = handle_link(
				*pkt,
				stat,
				&max_silence,
			);

			loop {
				// send keep alive if needed.
				if ka_time >= KEEPALIVE_FREQ_MS {
					stat.register_keepalive();
					ka_time = 0;
				}

				if sleep_time == 0 {
					break;
				}

				let time_til_ka = KEEPALIVE_FREQ_MS - ka_time;
				let micro_sleep_time = sleep_time
					.min(20)
					.min(time_til_ka);

				// Do I need to take keepalive time into account?
				stat.sleep(Duration::from_millis(micro_sleep_time));
				sleep_time -= micro_sleep_time;
				ka_time += micro_sleep_time;
			}
		}
	}

	// println!("{:?}", stats);
	let kbps = stats.iter().map(|(stat, _cap)| stat.mbps() * 1024.0).collect::<Vec<_>>();

	// println!("{:?} / 96kbps", kbps);

	let errs = stats.iter().map(|(stat, _cap)| stat.predictor_rms()).collect::<Vec<_>>();

	(kbps, errs[0])
}

#[inline]
fn handle_link(
	pkt: PacketChainLink,
	stat: &mut CallStats,
	max_silence: &Option<u64>,
) -> (u64, usize) {
	use PacketChainLink::*;

	let (mut sleep_time, pkt_size, prevent_feedback) = match pkt {
		Packet(p) => (0, usize::from(p), false),
		Missing(_t) => (0, stat.predict_voice_pkt() as usize, true),
		Silence(t) => {
			let out = u64::from(t);
			(out.min(max_silence.unwrap_or(out)), 0, false)
		},
	};

	sleep_time += 20;

	if pkt_size > 0 {
		stat.register_voice(pkt_size, prevent_feedback);
	}

	(sleep_time, pkt_size)
}


fn generate_ip4<R: Rng + ?Sized>(rng: &mut R, strat: IpStrategy) -> Ipv4Addr {
	let mut bytes = [0u8; 4];

	// First byte limited to between 1-223.
	// For simplicity, ensure we cannot start an IP with 10, 127, 172.
	// To keep uniformity, skip these.
	let a_distr = Uniform::new_inclusive(1, 220);
	bytes[0] = rng.sample(a_distr);

	if bytes[0] >= 10 {
		bytes[0] += 1;
	}
	if bytes[0] >= 127 {
		bytes[0] += 1;
	}
	if bytes[0] >= 172 {
		bytes[0] += 1;
	}

	// We can't prevent an IP from being broadcast/gateway unless we know what
	// the prefix length is. I'm assuming /32 prefix...
	for b in &mut bytes[1..] {
		*b = rng.gen();
	}

	match strat {
		IpStrategy::Even => {
			bytes[3] &= 0b1111_1110;
		},
		IpStrategy::Odd => {
			bytes[3] |= 0b0000_0001;
		},
		_ => {},
	}

	Ipv4Addr::from(bytes)
}
