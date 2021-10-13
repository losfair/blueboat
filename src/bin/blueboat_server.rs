#![no_main]

use blueboat::{pm_secure_init, server_main};
use nix::{
  libc,
  sys::signal::{signal, SigHandler, Signal},
};

#[no_mangle]
pub unsafe extern "C" fn main(
  argc: libc::c_int,
  argv: *mut *mut libc::c_char,
  envp: *mut *mut libc::c_char,
) -> i32 {
  // This would be handled by Rust's startup code but we have `no_main`.
  signal(Signal::SIGPIPE, SigHandler::SigIgn).unwrap();

  pm_secure_init(argc, argv, envp);
  server_main();
  0
}
