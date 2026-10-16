//! Containment: the cgroup v2 precondition (06 C6) and the keeper prep for container roots.
//!
//! Three classes, one probe — matching `fixtures/README.md` §Containment classes and the raw
//! evidence in `p2/cgroup-matrix/`:
//!
//! 1. **Unavailable** — no cgroup v2, or a cgroupfs that cannot be written. The probe fails
//!    with a detail string that lands in the pinned refusal (`containment_unavailable`), and
//!    the supported shape is the explicit downgrade `limits.mode = off`.
//! 2. **On, containerized** — the manager sits at its own cgroup namespace root (`0::/`) with a
//!    writable cgroupfs. Controllers cannot be enabled while the root holds processes (the
//!    cgroup v2 "no internal processes" rule under `nsdelegate` returns `EBUSY`), so the probe
//!    applies the **keeper prep**: move every pid from the root into a keeper child, then enable
//!    the controllers. (Raw sequence and outputs: `p2/cgroup-matrix/logs/cg-priv5.txt`.)
//! 3. **On, host-process** — the manager runs inside a delegated scope (e.g.
//!    `systemd-run --user … -p Delegate=yes`): the controllers are already delegated in the
//!    scope's `subtree_control`, so the probe just confirms them.
//!
//! The keeper dance is only ever performed at a cgroup **namespace root** (`0::/`). Inside a
//! delegated scope the root's processes are the user's whole session — moving them is not ours
//! to do, and a scope that does not delegate its controllers is class 1 (fix the wiring or set
//! `limits.mode = off`), never a prep candidate.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::manager::startup::{ContainmentProbe, Delegation};

/// Controllers the sessions need: memory + pids carry the limits, cpu fairness alongside.
const NEEDED_CONTROLLERS: [&str; 3] = ["cpu", "memory", "pids"];

/// EBUSY — the kernel's answer when controllers cannot be enabled under a process-holding
/// cgroup (the no-internal-processes rule).
const EBUSY: i32 = 16;

/// The detail used when the cgroupfs is mounted read-only (the wording the pinned refusal
/// carries as its example; `fixtures/ipc/manager-start-limits-unavailable.ron`).
pub const DETAIL_READ_ONLY: &str = "cgroupfs is mounted read-only";

/// The detail used when the cgroupfs exists but this process may not write it.
pub const DETAIL_NO_DELEGATION: &str = "no write access to the cgroup subtree (delegation missing)";

/// Parse the unified hierarchy path out of `/proc/self/cgroup` (`0::<path>`).
pub fn self_cgroup_path(raw: &str) -> Option<&str> {
    raw.lines()
        .find_map(|line| line.strip_prefix("0::"))
        .map(str::trim)
        .filter(|path| !path.is_empty())
}

/// The read-only/denied mapping for a failed probe write, kept as one function so the wording
/// is testable without a real read-only mount.
pub fn write_error_detail(error: &io::Error) -> String {
    match error.raw_os_error() {
        Some(30) => DETAIL_READ_ONLY.to_string(), // EROFS
        Some(13) | Some(1) => DETAIL_NO_DELEGATION.to_string(), // EACCES | EPERM
        _ => format!("cgroupfs is not writable ({error})"),
    }
}

/// Which controllers a `subtree_control` line enables (space-separated `+cpu` style or plain).
pub fn enabled_controllers(raw: &str) -> Vec<String> {
    raw.split_whitespace()
        .map(|token| token.trim_start_matches('+').to_string())
        .filter(|token| !token.is_empty())
        .collect()
}

fn covers_needed(controllers: &[String]) -> bool {
    NEEDED_CONTROLLERS
        .iter()
        .all(|needed| controllers.iter().any(|c| c == needed))
}

/// The production probe: reads the real filesystem defaults, but takes both its base and its
/// `/proc/self/cgroup` content by injection so unit tests exercise the decision logic on
/// tempdirs.
#[derive(Debug, Clone)]
pub struct LinuxCgroupProbe {
    base: PathBuf,
    self_cgroup: String,
}

impl Default for LinuxCgroupProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl LinuxCgroupProbe {
    pub fn new() -> Self {
        let self_cgroup = fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
        Self {
            base: PathBuf::from("/sys/fs/cgroup"),
            self_cgroup,
        }
    }

    /// For tests and for callers that manage cgroups beneath a different root.
    pub fn with_paths(base: impl Into<PathBuf>, self_cgroup: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            self_cgroup: self_cgroup.into(),
        }
    }

    /// The cgroup directory this process manages: the base at a namespace root (`0::/`), or
    /// the base plus the delegated path otherwise — exactly what the class rules act on.
    pub fn cgroup_dir(&self) -> Result<PathBuf, String> {
        let path = self_cgroup_path(&self.self_cgroup)
            .ok_or_else(|| "no unified cgroup v2 entry in /proc/self/cgroup".to_string())?;
        Ok(match path.trim_start_matches('/') {
            "" => self.base.clone(),
            rest => self.base.join(rest),
        })
    }

    fn is_namespace_root(&self) -> bool {
        self_cgroup_path(&self.self_cgroup).is_some_and(|path| path == "/")
    }

    /// Class 2's prep: move every pid out of the namespace root into a keeper child, then
    /// enable the controllers (the raw sequence: `p2/cgroup-matrix/logs/cg-priv5.txt`).
    fn keeper_prep_and_enable(&self, dir: &Path) -> Result<String, String> {
        fn drain(dir: &Path, keeper: &Path) {
            if let Ok(pids) = fs::read_to_string(dir.join("cgroup.procs")) {
                for pid in pids.split_whitespace() {
                    // A pid may exit between listing and moving; that is not an error.
                    let _ = fs::write(keeper.join("cgroup.procs"), pid);
                }
            }
        }
        fn enable(dir: &Path) -> io::Result<()> {
            fs::write(
                dir.join("cgroup.subtree_control"),
                format!("+{}\n", NEEDED_CONTROLLERS.join(" +")),
            )
        }

        let keeper = dir.join("keeper");
        fs::create_dir_all(&keeper).map_err(|e| format!("keeper child: {e}"))?;

        drain(dir, &keeper);
        let mut result = enable(dir);
        if result.as_ref().err().and_then(|e| e.raw_os_error()) == Some(EBUSY) {
            // A pid may have respawned between the drain and the write; drain once more.
            drain(dir, &keeper);
            result = enable(dir);
        }
        result.map_err(|e| format!("enable controllers after keeper prep: {e}"))?;

        let now = fs::read_to_string(dir.join("cgroup.subtree_control")).unwrap_or_default();
        let enabled = enabled_controllers(&now);
        if !covers_needed(&enabled) {
            return Err(format!(
                "controllers unavailable after namespace-root prep (present: {})",
                enabled.join(" ")
            ));
        }
        Ok(enabled.join(" "))
    }
}

impl ContainmentProbe for LinuxCgroupProbe {
    fn probe(&self) -> Result<Delegation, String> {
        let dir = self.cgroup_dir()?;

        // 1. Is there a cgroup v2 hierarchy here at all?
        if !dir.join("cgroup.controllers").exists() {
            return Err(format!("no cgroup v2 hierarchy at {}", dir.display()));
        }

        // 2. Writable? (The default container class fails here — read-only cgroupfs.)
        let probe_dir = dir.join(format!("daiorama-probe-{}", std::process::id()));
        match fs::create_dir(&probe_dir) {
            Ok(()) => {
                let _ = fs::remove_dir(&probe_dir);
            }
            Err(error) => return Err(write_error_detail(&error)),
        }

        // 3. Already delegated (the host-process class)? Confirm and be done.
        let current = fs::read_to_string(dir.join("cgroup.subtree_control")).unwrap_or_default();
        let enabled = enabled_controllers(&current);
        if covers_needed(&enabled) {
            return Ok(Delegation {
                detail: format!("delegated: {} already enabled", enabled.join(" ")),
            });
        }

        // 4. Try to enable at our level; at a namespace root with processes that is EBUSY,
        //    and the keeper prep is the way through (class 2).
        match fs::write(
            dir.join("cgroup.subtree_control"),
            format!("+{}\n", NEEDED_CONTROLLERS.join(" +")),
        ) {
            Ok(()) => {
                let now =
                    fs::read_to_string(dir.join("cgroup.subtree_control")).unwrap_or_default();
                let enabled = enabled_controllers(&now);
                // Mirror the keeper path (above): report only what the readback shows.
                if !covers_needed(&enabled) {
                    return Err(format!(
                        "controllers unavailable after enabling (present: {})",
                        enabled.join(" ")
                    ));
                }
                Ok(Delegation {
                    detail: format!("controllers enabled ({})", enabled.join(" ")),
                })
            }
            Err(error) if error.raw_os_error() == Some(EBUSY) && self.is_namespace_root() => {
                self.keeper_prep_and_enable(&dir).map(|list| Delegation {
                    detail: format!("controllers enabled after keeper prep ({list})"),
                })
            }
            Err(error) if error.raw_os_error() == Some(EBUSY) => Err(
                "controllers not delegated to this scope; run under a scope with Delegate=yes \
                 (e.g. systemd-run --user … -p Delegate=yes), or set limits.mode = off"
                    .to_string(),
            ),
            Err(error) => Err(format!("enabling controllers: {error}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Session scopes (the C6 wiring: per-session limits, cgroup.kill, the startup sweep)
// ---------------------------------------------------------------------------

/// The subtree under the manager's cgroup that owns this instance's session scopes (03:
/// "session cgroups live under an instance-owned subtree"; name provisional).
pub const SESSION_SUBTREE: &str = "daiorama-sessions";

/// Provisional per-session limits (06 C6 pins the mechanism, not the numbers — documented
/// provisionals, adjustable in one place when the review pins values).
pub const SESSION_MEMORY_MAX: &str = "4294967296"; // 4 GiB
pub const SESSION_CPU_MAX: &str = "200000 100000"; // 2 CPUs (quota/period, µs)
pub const SESSION_PIDS_MAX: &str = "512";

/// The instance session subtree under a manager cgroup directory.
pub fn session_subtree(manager_cgroup: &Path) -> PathBuf {
    manager_cgroup.join(SESSION_SUBTREE)
}

/// Create the subtree (idempotent) and enable the per-session controllers for its children.
/// The subtree holds no processes, so this cannot hit the no-internal-processes rule; the
/// readback is asserted the same way the keeper path asserts its own.
pub fn ensure_session_subtree(subtree: &Path) -> io::Result<()> {
    fs::create_dir_all(subtree)?;
    fs::write(
        subtree.join("cgroup.subtree_control"),
        format!("+{}\n", NEEDED_CONTROLLERS.join(" +")),
    )?;
    let now = fs::read_to_string(subtree.join("cgroup.subtree_control")).unwrap_or_default();
    let enabled = enabled_controllers(&now);
    if !covers_needed(&enabled) {
        return Err(io::Error::other(format!(
            "controllers unavailable on the session subtree (present: {})",
            enabled.join(" ")
        )));
    }
    Ok(())
}

/// Write one limit file with its path in the error (memory.max / cpu.max / pids.max).
pub fn write_limit(scope: &Path, file: &str, value: &str) -> io::Result<()> {
    let path = scope.join(file);
    fs::write(&path, value)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

/// Create one session scope with its provisional limits. On any failure nothing is left
/// half-created.
pub fn create_session_scope(subtree: &Path, id8: &str) -> io::Result<PathBuf> {
    let scope = subtree.join(id8);
    fs::create_dir(&scope)?;
    let result = write_limit(&scope, "memory.max", SESSION_MEMORY_MAX)
        .and_then(|_| write_limit(&scope, "cpu.max", SESSION_CPU_MAX))
        .and_then(|_| write_limit(&scope, "pids.max", SESSION_PIDS_MAX));
    if let Err(error) = result {
        let _ = fs::remove_dir(&scope);
        return Err(error);
    }
    Ok(scope)
}

/// `cgroup.kill` a scope, wait for the kernel to report it unpopulated (bounded), remove it.
/// This is the completion guarantee (03): every process in the scope — grandchildren and
/// browser process trees included — dies, whatever the parent links look like.
pub fn kill_scope(scope: &Path) -> io::Result<()> {
    let kill = scope.join("cgroup.kill");
    if !kill.exists() {
        // Nothing to kill (scope already gone or never populated); remove if still present.
        let _ = fs::remove_dir(scope);
        return Ok(());
    }
    let path = kill.display().to_string();
    fs::write(&kill, "1").map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match fs::read_to_string(scope.join("cgroup.events")) {
            Ok(events) if events.lines().any(|l| l.trim() == "populated 0") => break,
            Ok(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(_) => {
                return Err(io::Error::other(format!(
                    "scope {} still populated after cgroup.kill",
                    scope.display()
                )));
            }
            Err(_) => break, // the scope vanished under us — done
        }
    }
    fs::remove_dir(scope)
        .map_err(|e| io::Error::new(e.kind(), format!("removing scope {}: {e}", scope.display())))
}

/// The startup sweep (03): every session scope left under the subtree by a previous manager
/// run is `cgroup.kill`ed before this manager binds. An absent subtree is a no-op.
pub fn sweep_session_subtree(subtree: &Path) -> io::Result<u32> {
    let Ok(entries) = fs::read_dir(subtree) else {
        return Ok(0);
    };
    let mut swept = 0u32;
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        kill_scope(&entry.path())?;
        swept += 1;
    }
    Ok(swept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_cgroup_parsing() {
        assert_eq!(self_cgroup_path("0::/\n"), Some("/"));
        assert_eq!(
            self_cgroup_path("0::/user.slice/user-4242.slice/user@4242.service/run.scope\n"),
            Some("/user.slice/user-4242.slice/user@4242.service/run.scope")
        );
        assert_eq!(self_cgroup_path("2:cpu:/legacy\n"), None);
        assert_eq!(self_cgroup_path(""), None);
    }

    #[test]
    fn write_error_mapping_matches_the_pinned_wording() {
        assert_eq!(
            write_error_detail(&io::Error::from_raw_os_error(30)),
            DETAIL_READ_ONLY
        );
        assert_eq!(
            write_error_detail(&io::Error::from_raw_os_error(13)),
            DETAIL_NO_DELEGATION
        );
        assert_eq!(
            write_error_detail(&io::Error::from_raw_os_error(1)),
            DETAIL_NO_DELEGATION
        );
        assert!(write_error_detail(&io::Error::from_raw_os_error(5)).contains("not writable"));
    }

    #[test]
    fn subtree_control_parsing() {
        assert_eq!(
            enabled_controllers("cpu memory pids\n"),
            ["cpu", "memory", "pids"]
        );
        assert_eq!(enabled_controllers("+cpu +io\n"), ["cpu", "io"]);
        assert_eq!(enabled_controllers(""), Vec::<String>::new());
    }

    #[test]
    fn missing_hierarchy_is_class_one() {
        let tmp = tempfile::tempdir().unwrap();
        let probe = LinuxCgroupProbe::with_paths(tmp.path(), "0::/\n");
        let detail = probe.probe().unwrap_err();
        assert!(detail.contains("no cgroup v2 hierarchy"), "got: {detail}");
    }

    #[test]
    fn already_delegated_scope_confirms_without_prep() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("cgroup.controllers"),
            "cpu io memory pids\n",
        )
        .unwrap();
        fs::write(
            tmp.path().join("cgroup.subtree_control"),
            "cpu memory pids\n",
        )
        .unwrap();
        let probe = LinuxCgroupProbe::with_paths(tmp.path(), "0::/\n");
        let delegation = probe.probe().expect("delegation present");
        assert!(
            delegation.detail.contains("cpu memory pids"),
            "{}",
            delegation.detail
        );
        // No keeper child was needed on the host-process class.
        assert!(!tmp.path().join("keeper").exists());
    }

    #[test]
    fn unwritable_cgroupfs_is_class_one() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // Root ignores directory modes, so the EACCES branch cannot be simulated in a
        // root container; the wording mapping itself is covered by `write_error_mapping…`.
        let is_root = fs::metadata("/proc/self")
            .map(|meta| meta.uid() == 0)
            .unwrap_or(false);
        if is_root {
            eprintln!("skipping unwritable-dir assertion (running as root)");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("cgroup.controllers"),
            "cpu io memory pids\n",
        )
        .unwrap();
        let mut perms = fs::metadata(tmp.path()).unwrap().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(tmp.path(), perms).unwrap();
        let probe = LinuxCgroupProbe::with_paths(tmp.path(), "0::/\n");
        let detail = probe.probe().unwrap_err();
        assert_eq!(detail, DETAIL_NO_DELEGATION, "got: {detail}");
        // restore so tempdir cleanup can remove it
        let mut perms = fs::metadata(tmp.path()).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(tmp.path(), perms).unwrap();
    }

    #[test]
    fn delegated_scope_path_selects_the_scope_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let scope = tmp.path().join("user.slice/user-4242.slice/run.scope");
        fs::create_dir_all(&scope).unwrap();
        fs::write(scope.join("cgroup.controllers"), "cpu io memory pids\n").unwrap();
        fs::write(scope.join("cgroup.subtree_control"), "cpu memory pids\n").unwrap();
        let probe =
            LinuxCgroupProbe::with_paths(tmp.path(), "0::/user.slice/user-4242.slice/run.scope\n");
        let delegation = probe.probe().expect("delegation present");
        assert!(delegation.detail.contains("already enabled"));
    }

    /// The real-box check: runs the probe against the actual `/sys/fs/cgroup`. Run explicitly
    /// inside the two container classes from `p2/cgroup-matrix/`:
    ///
    /// - unprivileged container → `PROBE-ERR: cgroupfs is mounted read-only` (class 1);
    /// - privileged container → keeper prep runs → `PROBE-OK: … keeper prep (cpu memory pids)`
    ///   (class 2).
    ///
    /// `cargo test --locked real_box_probe -- --ignored --nocapture`
    #[test]
    #[ignore = "touches the real cgroupfs; run explicitly"]
    fn real_box_probe() {
        let probe = LinuxCgroupProbe::new();
        match probe.probe() {
            Ok(delegation) => println!("PROBE-OK: {}", delegation.detail),
            Err(detail) => println!("PROBE-ERR: {detail}"),
        }
    }

    #[test]
    fn session_subtree_lives_under_the_manager_cgroup() {
        let subtree = session_subtree(Path::new("/sys/fs/cgroup"));
        assert_eq!(subtree, Path::new("/sys/fs/cgroup/daiorama-sessions"));
        assert!(
            SESSION_MEMORY_MAX.parse::<u64>().is_ok(),
            "memory.max is bytes"
        );
        assert_eq!(
            SESSION_CPU_MAX.split_whitespace().count(),
            2,
            "cpu.max is quota + period"
        );
        assert!(SESSION_PIDS_MAX.parse::<u32>().is_ok());
    }

    #[test]
    fn sweep_is_a_noop_without_a_subtree_and_skips_non_scopes() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            sweep_session_subtree(&tmp.path().join("absent")).unwrap(),
            0,
            "an absent subtree is a no-op (03)"
        );
        fs::write(tmp.path().join("stray-file"), "x").unwrap();
        assert_eq!(
            sweep_session_subtree(tmp.path()).unwrap(),
            0,
            "files are not scopes"
        );
    }
}
