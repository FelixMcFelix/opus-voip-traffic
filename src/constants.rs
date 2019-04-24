pub const CMAC_BYTES: usize = 16;
pub const RTP_BYTES: usize = 12;
pub const KEEPALIVE_SIZE: usize = 8;

// Note: assume no options in IPv4...
pub const ETH_HEADER_LEN: usize = 14;
pub const IPV4_HEADER_LEN: usize = 20;
pub const UDP_HEADER_LEN: usize = 8;

pub const KEEPALIVE_FREQ_MS: u64 = 5000;
pub const TRACE_DIR: &str = "traces/";
