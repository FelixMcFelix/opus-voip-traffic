mod packet_format;

use crate::constants::*;

use parking_lot::RwLock;
use rayon::prelude::*;
use speedy::{Endianness, Readable, Writable};
use std::{
	fmt::Debug,
	fs::{
		self,
		File,
	},
	path::PathBuf,
	sync::Arc,
};

pub(crate) use packet_format::*;

pub type OldTrace = Vec<OldPacketChainLink>;
pub type Trace = Vec<PacketChainLink>;
pub type MemoTrace = Arc<RwLock<Option<Trace>>>;

pub struct TraceHolder {
	paths: Vec<PathBuf>,
	// Use RW lock here because we only want to enter *once*.
	data: Vec<MemoTrace>,
}

impl TraceHolder {
	fn new(paths: Vec<PathBuf>) -> Self {
		let mut data = Vec::with_capacity(paths.len());
		
		for _ in 0..paths.len() {
			data.push(Arc::new(RwLock::new(None)));
		}

		Self {
			paths,
			data,
		}
	}

	pub fn len(&self) -> usize {
		self.paths.len()
	}

	pub fn get_trace(&self, i: usize) -> MemoTrace {
		// IDEA:
		// * Try to acquire a Write. 
		// * If fail, guaranteed to be safe to use a Read later.
		//   (Why? Either a write in progress, or reads in progress).
		// * If succ, check contents.
		//   * If good, release lock IMMEDIATELY.
		//   * If bad, fill it in.

		let locked_value = self.data[i].clone();
		if let Some(mut value) = locked_value.try_write() {
			// Got write lock, NOW do some thinking.
			// Drop the lock ASAP if Some(trace).
			if value.is_none() {
				*value = Some(File::open(&self.paths[i])
					.ok()
					.and_then(|f| Trace::read_from_stream(Endianness::LittleEndian, f).ok())
					.unwrap()
				);
			}
		}

		locked_value
	}

	pub fn get_path(&self, i: usize) -> Option<&PathBuf> {
		self.paths.get(i)
	}
}

pub fn convert_all_traces(base_dir: &str) {
	let target_dir = format!("{}/{}", base_dir, SPEEDY_TRACE_DIR);
	fs::create_dir_all(&target_dir)
		.expect("Need a directory to write trace files to...");

	let data = read_traces(base_dir);
	data.par_iter()
		.for_each(|(trace, path)| {
			let f = File::create(
				&format!("{}/{}",
					&target_dir,
					path.file_name()
						.as_ref()
						.expect("File name should correspond.")
						.to_string_lossy()
				))
				.expect("Couldn't open file...");

			let o: Trace = trace.iter().map(Into::into).collect();

			o.write_to_stream(Endianness::LittleEndian, f)
				.expect("Somehow couldn't serialise?");
		});
}

pub fn read_traces_memo(base_dir: &str) -> TraceHolder {
	let file_entries: Vec<PathBuf> = fs::read_dir(&format!("{}/{}", base_dir, SPEEDY_TRACE_DIR))
		.expect("Couldn't read files in trace directory...")
		.filter_map(|x| if let Ok(x) = x {
			Some(x.path())
		} else {
			warn!("Couldn't read entry: {:?}", x);
			None
		}).collect();

	TraceHolder::new(file_entries)
}

pub fn read_traces(base_dir: &str) -> Vec<(OldTrace, PathBuf)> {
	let file_entries: Vec<PathBuf> = fs::read_dir(&format!("{}/{}", base_dir, TRACE_DIR))
		.expect("Couldn't read files in trace directory...")
		.filter_map(|x| if let Ok(x) = x {
			Some(x.path())
		} else {
			warn!("Couldn't read entry: {:?}", x);
			None
		}).collect();

	file_entries.par_iter()
		.map(File::open)
		.filter_map(warn_and_unpack)
		.map(bincode::deserialize_from)
		.filter_map(warn_and_unpack)
		.collect::<Vec<OldTrace>>()
		.into_iter()
		.zip(file_entries)
		.filter(|x: &(OldTrace, PathBuf)| !x.0.is_empty())
		.collect()
}

fn warn_and_unpack<T, E: Debug>(input: Result<T, E>) -> Option<T> {
	input.map_err(|why| {
		warn!("Couldn't parse entry: {:?}", why);
		why
	}).ok()
}

pub fn largest_packet(trace: &[PacketChainLink]) -> Option<u16> {
	let mut out = None;
	for part in trace {
		if let PacketChainLink::Packet(size) = part {
			let size = size;
			let target = out.get_or_insert(*size);

			*target = *size.max(target);
		}
	}

	out
}
