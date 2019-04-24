use clap::{App, Arg};
use opus_voip_traffic::{
	Config,
	IpStrategy,
};
use pnet::datalink;
use std::{
	net::ToSocketAddrs,
	time::Duration,
};

fn main() {
	env_logger::init();

	let matches =
		App::new("Opus VOIP traffic Generator")
			.version("0.1.0")
			.author("Kyle Simpson <k.simpson.1@research.gla.ac.uk>")
			.about("Generate UDP traffic matching the distribution of Opus VOIP traffic")
			// Show config?
			.arg(Arg::with_name("show-config")
				.short("S")
				.long("show-config")
				.help("Display program's configuration."))

			// Base dir.
			.arg(Arg::with_name("base-dir")
				.short("b")
				.long("base-dir")
				.value_name("DIR")
				.help("Path to this program's base folder.")
				.takes_value(true)
				.default_value("."))

			// Connectivity / main operation.
			// FIXME: allow multiple IPs to be offered.
			.arg(Arg::with_name("ip")
				.short("i")
				.long("ip")
				.value_name("IP")
				.help("Server to send requests to.")
				.takes_value(true)
				.default_value("10.0.0.1"))
			.arg(Arg::with_name("port")
				.short("p")
				.long("port")
				.value_name("PORT")
				.help("Target port for UDP traffic.")
				.takes_value(true)
				.default_value("50864"))
			.arg(Arg::with_name("server")
				.short("s")
				.long("server")
				.help("Run in server mode."))
			.arg(Arg::with_name("iface")
				.long("iface")
				.help("Enable IP randomisation over target interface.")
				.value_name("INTERFACE")
				.takes_value(true))
			.arg(Arg::with_name("ip-strategy")
				.long("ip-strategy")
				.help("Force generated IPs (from --iface) to be generated according to a known pattern ('even', 'odd').")
				.value_name("PATTERN_NAME")
				.takes_value(true))

			// Call timing configs.
			.arg(Arg::with_name("max-silence")
				.short("m")
				.long("max-silence")
				.value_name("MAX_SILENCE")
				.help("Maximum duration to remain silent for (ms).")
				.takes_value(true))
			.arg(Arg::with_name("duration-lb")
				.short("l")
				.long("duration-lb")
				.value_name("DURATION_LB")
				.help("Minimum duration of communication (ms).")
				.takes_value(true)
				.default_value("0"))
			.arg(Arg::with_name("duration-ub")
				.short("u")
				.long("duration-ub")
				.value_name("DURATION_UB")
				.help("Maximum duration of communication (ms).")
				.takes_value(true))
			.arg(Arg::with_name("randomise")
				.short("r")
				.long("randomise")
				.help("Randomise call duration."))
			.arg(Arg::with_name("constant")
				.long("constant")
				.help("Constantly generate traffic."))
			.arg(Arg::with_name("refresh")
				.long("refresh")
				.help("Generate new IP/Port/SSRC on call end."))

			// Concurrent execution strains.
			.arg(Arg::with_name("thread-count")
				.short("c")
				.long("thread-count")
				.value_name("THREAD_COUNT")
				.help("Amount of concurrent calls to host (client).")
				.takes_value(true)
				.default_value("1"))

			// Dry-run mode, for stats gathering.
			.arg(Arg::with_name("stats")
				.long("stats")
				.help("Generate overall trace statistics."))

			// Server configs.
			.arg(Arg::with_name("min-room-size")
				.long("min-room-size")
				.value_name("N")
				.help("Minimum amount of callers to place into a room.")
				.takes_value(true)
				.default_value("2"))
			.arg(Arg::with_name("max-room-size")
				.long("max-room-size")
				.value_name("N")
				.help("Maximum amount of callers to place into a room.")
				.takes_value(true)
				.default_value("8"))
			.arg(Arg::with_name("single-room")
				.long("single-room")
				.help("Place all callers into a single room."))

			.get_matches();

	let ip = matches.value_of_lossy("ip")
		.expect("Ip always guaranteed to exist.");

	let port = matches.value_of_lossy("port")
		.expect("Port always guaranteed to exist.")
		.parse::<u16>()
		.expect("Port must be in range of 16-bit uint.");

	let address = (ip.as_ref(), port)
		.to_socket_addrs()
		.expect("Server + port combination are invalid!")
		.next().unwrap();

	let interface = matches.value_of_lossy("iface")
		.map(|if_name|
			datalink::interfaces().iter()
				.find(move |iface| iface.name == if_name)
				.unwrap_or_else(||
					panic!(
						"Interface name wasn't valid: consider one of {:?}.",
						datalink::interfaces().into_iter().map(|iface| iface.name).collect::<Vec<_>>(),
					)
				)
				.clone()
		);

	let ip_modifier = matches.value_of_lossy("ip-strategy")
		.and_then(|name| match &*name {
			"even" => Some(IpStrategy::Even),
			"odd" => Some(IpStrategy::Odd),
			_ => None,
		})
		.unwrap_or(IpStrategy::Vanilla);

	let max_silence = matches.value_of_lossy("max-silence")
		.map(|s|
			s.parse::<u64>()
				.expect("Max silence must be an integer."));

	let duration_lb = matches.value_of_lossy("duration-lb")
		.map(|s| Duration::from_millis(
			s.parse::<u64>()
				.expect("Duration lower bound must be an integer.")
		))
		.expect("Duration lower bound guaranteed to exist.");

	let duration_ub = matches.value_of_lossy("duration-ub")
		.map(|s| Duration::from_millis(
			s.parse::<u64>()
				.expect("Duration upper bound must be an integer.")
		));

	let randomise_duration = matches.is_present("randomise");

	let constant = matches.is_present("constant");

	let refresh = matches.is_present("refresh");

	let thread_count = matches.value_of_lossy("thread-count")
		.expect("Thread count always guaranteed to exist.")
		.parse::<usize>()
		.expect("Thread count must be an integer.");

	let min_room_size = matches.value_of_lossy("min-room-size")
		.expect("Room minimum always guaranteed to exist.")
		.parse::<usize>()
		.expect("Room minimum must be an integer.");

	let max_room_size = matches.value_of_lossy("max-room-size")
		.expect("Room maximum always guaranteed to exist.")
		.parse::<usize>()
		.expect("Room maximum must be an integer.");

	let base_dir = matches.value_of_lossy("base-dir")
		.expect("Base directory always guaranteed to exist.")
		.to_string();

	let split_rooms = !matches.is_present("single-room");

	let config = Config {
		base_dir,

		address,
		port,
		interface,
		ip_modifier,

		max_silence,
		duration_lb,
		duration_ub,
		randomise_duration,
		constant,
		refresh,

		thread_count,

		min_room_size,
		max_room_size,
		split_rooms,
	};

	if matches.is_present("show-config") {
		println!("{:#?}", config);
	}

	if matches.is_present("server") {
		opus_voip_traffic::server(&config);
	} else if matches.is_present("stats") {
		opus_voip_traffic::gen_stats(&config);
	} else {
		opus_voip_traffic::client(&config);
	}
}
