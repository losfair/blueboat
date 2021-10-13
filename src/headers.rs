pub const HDR_GLOBAL_PREFIX: &str = "x-blueboat-";

pub const HDR_REQ_REQUEST_ID: &str = "x-blueboat-request-id";
pub const HDR_REQ_METADATA: &str = "x-blueboat-metadata";
pub const HDR_REQ_CLIENT_IP: &str = "x-blueboat-client-ip";

pub const HDR_REQ_CLIENT_COUNTRY: &str = "x-blueboat-client-country";
pub const HDR_REQ_CLIENT_SUBDIVISION_PREFIX: &str = "x-blueboat-client-subdivision-";
pub const HDR_REQ_CLIENT_CITY: &str = "x-blueboat-client-city";

pub const HDR_REQ_CLIENT_WPBL: &str = "x-blueboat-client-wpbl";

pub const HDR_RES_HANDLE_LATENCY: &str = "x-blueboat-handle-latency";
pub const HDR_RES_BUSY_DURATION: &str = "x-blueboat-busy-duration";
pub const HDR_RES_REQUEST_ID: &str = "x-blueboat-request-id";

pub static PROXY_HEADER_WHITELIST: phf::Set<&'static str> = phf::phf_set! {
  "x-blueboat-request-id",
  "x-blueboat-metadata",
  "x-blueboat-client-ip",
};
