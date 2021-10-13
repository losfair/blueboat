use nix::libc;

/// Enable seccomp from the first thread of a newly-created process.
///
/// The caller must be the first thread of a process.
pub fn enable_seccomp_from_first_thread() {
  init_seccomp();
  log::info!("Enabled seccomp.");
}

fn init_seccomp() {
  use syscallz::{Action, Cmp, Comparator, Context, Syscall};

  static SYSCALL_LIST: &'static str = include_str!("../secure_mode_syscalls.json");

  let mut ctx = Context::init_with_action(Action::KillProcess).unwrap();
  let allow: Vec<String> = serde_json::from_str(SYSCALL_LIST).unwrap();
  for name in &allow {
    let syscall =
      Syscall::from_name(name).unwrap_or_else(|| panic!("allow syscall `{}` not found", name));
    ctx.allow_syscall(syscall).unwrap();
  }

  // Special case for `clone`: Follow the Docker rule.
  // https://github.com/moby/moby/blob/1430d849a4fe74d601896d4bbb0134e898ef8a76/profiles/seccomp/default.json#L587
  ctx
    .set_rule_for_syscall(
      Action::Allow,
      Syscall::clone,
      &[Comparator::new(0, Cmp::MaskedEq, 2114060288, Some(0))],
    )
    .unwrap();

  // Special case for `openat`: read-only.
  {
    let mask = !((libc::O_RDONLY | libc::O_CLOEXEC) as u32);
    ctx
      .set_rule_for_syscall(
        Action::Allow,
        Syscall::openat,
        &[Comparator::new(2, Cmp::MaskedEq, mask as u64, Some(0))],
      )
      .unwrap();
  }

  // Special case for `bind`: Return an error code.
  ctx
    .set_action_for_syscall(Action::Errno(1), Syscall::bind)
    .unwrap();

  ctx.load().unwrap();
  log::debug!("Seccomp policy loaded. allow={:?}", allow);
}
