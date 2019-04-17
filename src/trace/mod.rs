mod packet_format;

use parking_lot::RwLock;
use rayon::prelude::*;
use std::{
	fmt::Debug,
	fs::{
		self,
		File,
	},
	path::PathBuf,
	time::Instant,
	sync::Arc,
};

pub(crate) use packet_format::*;

const TRACE_DIR: &str = "traces/";

pub type Trace = Vec<PacketChainLink>;
pub type MemoTrace = Arc<RwLock<Option<Trace>>>;

pub struct TraceHolder {
	paths: Vec<PathBuf>,
	// Use RW lock here because we only want to enter *once*.
	data: Vec<MemoTrace>,
}

impl TraceHolder {
	fn new(paths: Vec<PathBuf>) -> Self {
		let mut data = Vec::new();
		data.resize(paths.len(), Arc::new(RwLock::new(None)));

		Self {
			paths,
			data,
		}
	}

	pub fn len(&self) -> usize {
		self.paths.len()
	}

	pub fn get_trace<'a>(&self, i: usize) -> MemoTrace {
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
					.and_then(|f| bincode::deserialize_from(f).ok())
					.unwrap()
				);
			}
		}

		locked_value
	}
}

pub fn read_traces_memo(base_dir: &String) -> TraceHolder {
	let file_entries: Vec<PathBuf> = fs::read_dir(&format!("{}/{}", base_dir, TRACE_DIR))
		.expect("Couldn't read files in trace directory...")
		.filter_map(|x| if let Ok(x) = x {
			Some(x.path())
		} else {
			warn!("Couldn't read entry: {:?}", x);
			None
		}).collect();

	TraceHolder::new(file_entries)
}

pub fn read_traces(base_dir: &String) -> Vec<Trace> {
	let file_entries: Vec<PathBuf> = fs::read_dir(&format!("{}/{}", base_dir, TRACE_DIR))
		.expect("Couldn't read files in trace directory...")
		.filter_map(|x| if let Ok(x) = x {
			Some(x.path())
		} else {
			warn!("Couldn't read entry: {:?}", x);
			None
		}).collect();

	//file_entries.par_iter()
	file_entries.iter()
		.map(File::open)
		.filter_map(warn_and_unpack)
		.map(bincode::deserialize_from)
		.filter_map(warn_and_unpack)
		.filter(|x: &Trace| !x.is_empty())
		.collect()
}

fn warn_and_unpack<T, E: Debug>(input: Result<T, E>) -> Option<T> {
	input.map_err(|why| {
		warn!("Couldn't parse entry: {:?}", why);
		why
	}).ok()
}
