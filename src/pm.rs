use std::cell::RefCell;
use std::process::Command;

use nix::{
  libc,
  sys::{
    signal::{signal, SigHandler, Signal},
    stat::{Mode, SFlag},
  },
  unistd::Uid,
};
use parking_lot::Mutex;
use smr::pm::{pm_secure_start, PmHandle};
use tempdir::TempDir;

use crate::{
  bootstrap::JSLAND_SNAPSHOT, gres::load_global_resources_single_threaded, ipc::BlueboatIpcReq,
  secure_mode::enable_seccomp_from_first_thread, v8util::set_up_v8_globally,
};

static mut PM: Option<Mutex<PmHandle<BlueboatIpcReq>>> = None;

thread_local! {
  static ISOLATE_BUFFER: RefCell<Option<v8::OwnedIsolate>> = RefCell::new(None);
}

pub fn take_isolate() -> v8::OwnedIsolate {
  ISOLATE_BUFFER.with(|cell| cell.borrow_mut().take().expect("isolate already taken"))
}

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

    set_up_v8_globally();
    ISOLATE_BUFFER.with(|buf| {
      *buf.borrow_mut() = Some(v8::Isolate::new(
        v8::CreateParams::default().snapshot_blob(JSLAND_SNAPSHOT),
      ));
    });
    log::info!("V8 initialized.");

    // Now we have sanitized our environment, we are a little bit freeier to load resources.
    // But still keep security in mind!
    load_global_resources_single_threaded();

    if is_seccomp_disabled() {
      log::warn!("Seccomp is disabled.");
      let mut insecure = true;

      match std::env::var("SMRAPP_BLUEBOAT_DANGEROUSLY_NO_SETUID") {
        Ok(x) if x == "1" => {
          log::warn!("setuid disabled");
        }
        _ => {
          if nix::unistd::getuid().is_root() {
            let temp_root = TempDir::new("blueboat-pm").unwrap().into_path();
            log::info!("Created temporary root: {}", temp_root.to_string_lossy());
            std::fs::create_dir_all(temp_root.join("etc")).unwrap();
            std::fs::create_dir_all(temp_root.join("etc/ssl/certs")).unwrap();
            std::fs::create_dir_all(temp_root.join("usr/lib")).unwrap();

            // smr defaults SIGCHLD to SigIgn which prevents `Command::new` from working
            signal(Signal::SIGCHLD, SigHandler::SigDfl).unwrap();

            match Command::new("mount")
              .arg("-o")
              .arg("bind,ro")
              .arg("/usr/lib")
              .arg(temp_root.join("usr/lib"))
              .status()
            {
              Ok(x) if x.success() => {
                log::info!(
                  "Mounted /usr/lib to {}",
                  temp_root.join("usr/lib").to_string_lossy()
                );
              }
              Ok(x) => {
                log::error!(
                  "Failed to mount /usr/lib to {}: {}",
                  temp_root.join("usr/lib").to_string_lossy(),
                  x
                );
              }
              Err(e) => {
                log::error!("Failed to call \"mount\": {}", e);
              }
            }

            signal(Signal::SIGCHLD, SigHandler::SigIgn).unwrap();

            if let Err(e) = std::fs::copy("/etc/resolv.conf", temp_root.join("etc/resolv.conf")) {
              log::warn!("Failed to copy resolv.conf: {}", e);
            }
            if let Err(e) = std::fs::copy(
              "/etc/ssl/certs/ca-certificates.crt",
              temp_root.join("etc/ssl/certs/ca-certificates.crt"),
            ) {
              log::warn!("Failed to copy ca-certificates.crt: {}", e);
            }
            if let Err(e) = nix::unistd::chroot(&temp_root) {
              log::error!(
                "cannot chroot into temporary directory - maybe not very safe: {:?}",
                e
              );
            } else {
              nix::unistd::chdir("/").unwrap();
              std::fs::create_dir_all("/dev").unwrap();
              nix::sys::stat::mknod(
                "/dev/null",
                SFlag::S_IFCHR,
                Mode::from_bits_truncate(0o666),
                nix::sys::stat::makedev(1, 3),
              )
              .unwrap();
              nix::sys::stat::mknod(
                "/dev/zero",
                SFlag::S_IFCHR,
                Mode::from_bits_truncate(0o666),
                nix::sys::stat::makedev(1, 5),
              )
              .unwrap();
              nix::sys::stat::mknod(
                "/dev/random",
                SFlag::S_IFCHR,
                Mode::from_bits_truncate(0o666),
                nix::sys::stat::makedev(1, 8),
              )
              .unwrap();
              nix::sys::stat::mknod(
                "/dev/urandom",
                SFlag::S_IFCHR,
                Mode::from_bits_truncate(0o666),
                nix::sys::stat::makedev(1, 9),
              )
              .unwrap();
            }
            nix::unistd::setuid(Uid::from_raw(1)).unwrap();
            log::info!("Using user-based isolation.");
            insecure = false;
          }
        }
      }

      if insecure {
        log::error!("This deployment is NOT secure because permission isolation between processes is not possible.");
      }
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
