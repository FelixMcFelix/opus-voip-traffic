use serde::{
	Deserialize,
	Serialize,
};
use std::num::NonZeroU16;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum OldPacketChainLink {
	Packet(NonZeroU16),
	Missing(u16),
	Silence(u32),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Readable, Writable)]
pub enum PacketChainLink {
	Packet(u16),
	Missing(u16),
	Silence(u32),
}

impl From<OldPacketChainLink> for PacketChainLink {
	fn from(a: OldPacketChainLink) -> PacketChainLink {
		match a {
			OldPacketChainLink::Packet(p) => PacketChainLink::Packet(p.get()),
			OldPacketChainLink::Missing(p) => PacketChainLink::Missing(p),
			OldPacketChainLink::Silence(p) => PacketChainLink::Silence(p),
		}
	}
}

impl From<&OldPacketChainLink> for PacketChainLink {
	fn from(a: &OldPacketChainLink) -> PacketChainLink {
		match a {
			OldPacketChainLink::Packet(p) => PacketChainLink::Packet(p.get()),
			OldPacketChainLink::Missing(p) => PacketChainLink::Missing(*p),
			OldPacketChainLink::Silence(p) => PacketChainLink::Silence(*p),
		}
	}
}