use nix::libc;
use parking_lot::Mutex;
use smr::pm::{pm_secure_start, PmHandle};

use crate::{
  gres::load_global_resources_single_threaded, ipc::BlueboatIpcReq,
  secure_mode::enable_seccomp_from_first_thread,
};

static mut PM: Option<Mutex<PmHandle<BlueboatIpcReq>>> = None;

pub unsafe fn secure_init(
  argc: libc::c_int,
  argv: *mut *mut libc::c_char,
  envp: *mut *mut libc::c_char,
) {
  PM = Some(Mutex::new(pm_secure_start(argc, argv, envp, || {
    tracing_subscriber::fmt().init();

    if let Ok(x) = std::env::var("SMRAPP_BLUEBOAT_HTTP_PROXY") {
      std::env::set_var("HTTP_PROXY", &x);
    }

    if let Ok(x) = std::env::var("SMRAPP_BLUEBOAT_HTTPS_PROXY") {
      std::env::set_var("HTTPS_PROXY", &x);
    }

    if let Ok(x) = std::env::var("SMRAPP_BLUEBOAT_NO_PROXY") {
      std::env::set_var("NO_PROXY", &x);
    }

    log::info!(
      "Visible env vars: {:?}",
      std::env::vars().collect::<Vec<_>>()
    );

    // Now we have sanitized our environment, we are a little bit freeier to load resources.
    // But still keep security in mind!
    load_global_resources_single_threaded();

    if is_seccomp_disabled() {
      log::warn!("Seccomp is disabled.");
    } else {
      enable_seccomp_from_first_thread();
    }
  })));
}

pub fn pm_handle() -> PmHandle<BlueboatIpcReq> {
  unsafe { PM.as_ref().unwrap().lock().clone() }
}

pub fn is_seccomp_disabled() -> bool {
  std::env::var("SMRAPP_BLUEBOAT_DISABLE_SECCOMP")
    .ok()
    .map(|x| x == "1")
    .unwrap_or(false)
}
