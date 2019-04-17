use serde::{
	Deserialize,
	Serialize,
};
use std::num::NonZeroU16;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum PacketChainLink {
	Packet(NonZeroU16),
	Missing(u16),
	Silence(u32),
}
