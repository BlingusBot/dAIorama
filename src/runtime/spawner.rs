//! Spawner: the L1 chain as a daemon-managed child — mutter `--headless` parent, nested
//! Hyprland, on-demand headless output at the requested resolution and scale.
//!
//! **P2 / W2, slice 2.** The chain is the one proven in P1 L1 (`p1/scripts/run-l1.sh`, goldens
//! under `p1/evidence/`): mutter's native headless backend renders on the render node with no
//! KMS and no seat, Hyprland nests on it via its Wayland backend, and outputs are created on
//! demand. This slice makes it a *managed* spawn: readiness waits with timeouts, a process
//! table, and teardown that leaves nothing behind.
//!
//! Semantics pinned elsewhere and reused here: the creation parameters are logical resolution
//! plus scale, and the physical mode is `mode = logical × scale` (the fixtures pin the wire,
//! the spawner owns this arithmetic).
//!
//! Deliberate slice scope: tests drive the machinery with **stubbed binaries**; the real
//! compositors land with the serve wiring inside the container image from `p1/l1/Containerfile`.
//! Containment attaches here: every managed child is forked on a dedicated **forker** thread —
//! the kernel binds the parent-death signal to the *forking thread*, so worker threads never
//! fork children — with a `pre_exec` PEP (`PR_SET_PDEATHSIG`, a `getppid` re-check, and the
//! session-scope self-join when containment is on). Transient clients (hyprctl, diagnostics)
//! are not managed children and stay direct spawns.

use std::collections::HashMap;
use std::ffi::CString;
use std::fs::{self, File};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::ipc::ErrorEnvelope;
use crate::manager::{Resolution, SessionSpawner, SessionSpec, SessionState};
use crate::runtime::containment::{create_session_scope, kill_scope};

/// The parent's wayland socket name inside each session's runtime dir.
pub const PARENT_SOCKET: &str = "wayland-m";

/// The output the spawner creates on the child compositor.
pub const PRIMARY_OUTPUT: &str = "HEADLESS-1";

/// Unix socket paths are capped at 108 bytes (`sun_path`); Hyprland's instance-directory
/// name runs ~61-64 chars and its IPC socket is `rt_dir + "/hypr/<instance>/.socket.sock"`.
/// Small margin kept for variance.
const SUN_PATH_BUDGET: usize = 106;
const HYPR_IPC_SUFFIX: usize = 6 + 64 + 13; // "/hypr/" + instance dir + "/.socket.sock"

/// The first candidate whose `dr-<id8>` child keeps the IPC socket under the unix
/// `sun_path` cap; `None` when every candidate is too deep.
fn first_fitting_runtime_dir(candidates: &[PathBuf], id: &str) -> Option<PathBuf> {
    candidates.iter().find_map(|base| {
        let rt_dir = base.join(format!("dr-{}", &id[..8]));
        (rt_dir.display().to_string().len() + HYPR_IPC_SUFFIX <= SUN_PATH_BUDGET).then_some(rt_dir)
    })
}

/// Containment wiring for the sessions this spawner manages (the C6 attach).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Containment {
    /// No containment: the compositors are plain direct children — today's behavior.
    #[default]
    Off,
    /// Sessions attach to per-session cgroup scopes under this subtree (the manager
    /// instance's subtree: `containment::session_subtree` of the manager cgroup dir).
    On { subtree: PathBuf },
}

/// The scope-directory name for a session: the first 8 characters of the session id (the
/// convention `containment::create_session_scope` takes as its `id8`).
fn scope_name(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

/// The scope directory a session attaches to under a containment mode, or `None` when
/// containment is off.
fn session_scope_path(containment: &Containment, id: &str) -> Option<PathBuf> {
    match containment {
        Containment::Off => None,
        Containment::On { subtree } => Some(subtree.join(scope_name(id))),
    }
}

/// Binaries and paths the spawner drives. Tests point these at stubs.
#[derive(Debug, Clone)]
pub struct SpawnerConfig {
    pub mutter_bin: String,
    pub hyprland_bin: String,
    pub hyprctl_bin: String,
    pub runtime_root: PathBuf,
    /// Where the compositors' `XDG_RUNTIME_DIR` lives. Kept tiny and separate from the
    /// session dir: unix socket paths are capped at 108 bytes (`sun_path`) and Hyprland's
    /// instance-directory name alone runs ~61 chars, so the runtime dir must stay short.
    /// When this base cannot fit the budget, `/tmp` then `$XDG_RUNTIME_DIR` are tried —
    /// the spawner refuses only when no candidate fits.
    pub short_root: PathBuf,
    pub parent_ready_timeout: Duration,
    pub child_ready_timeout: Duration,
    /// Containment wiring for session children; `Off` (the default) keeps today's behavior.
    pub containment: Containment,
}

impl SpawnerConfig {
    pub fn new(runtime_root: impl Into<PathBuf>) -> Self {
        Self {
            mutter_bin: "mutter".to_string(),
            hyprland_bin: "Hyprland".to_string(),
            hyprctl_bin: "hyprctl".to_string(),
            runtime_root: runtime_root.into(),
            short_root: std::env::temp_dir(),
            parent_ready_timeout: Duration::from_secs(15),
            child_ready_timeout: Duration::from_secs(30),
            containment: Containment::Off,
        }
    }
}

#[derive(Debug)]
struct ManagedSession {
    dir: PathBuf,
    rt_dir: PathBuf,
    /// The session's cgroup scope when containment is on; `None` when off.
    scope: Option<PathBuf>,
    parent: Child,
    child: Child,
    signature: String,
}

/// What one health pass saw (06 B3 inputs): both processes' liveness and whether the child
/// compositor answers a trivial IPC query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthReport {
    pub parent_alive: bool,
    pub child_alive: bool,
    pub compositor_answers: bool,
}

impl HealthReport {
    /// The registry state this report implies (06 B3 ladder): anything dead → `Dead`; a live
    /// tree whose compositor does not answer → `Degraded` (transient; the manager engages the
    /// kill path); both alive and answering → the healthy resting state, `Running`.
    pub fn state(&self) -> SessionState {
        if !self.parent_alive || !self.child_alive {
            SessionState::Dead
        } else if !self.compositor_answers {
            SessionState::Degraded
        } else {
            SessionState::Running
        }
    }
}

/// One spawn request for the forker thread: the prepared command, and the channel the
/// thread answers on.
struct ForkRequest {
    command: Command,
    reply: mpsc::Sender<io::Result<Child>>,
}

/// The dedicated thread that owns every managed child fork.
///
/// The kernel binds the parent-death signal to the *thread* that forked the child: if that
/// thread exits, the child is signalled even while the rest of the manager lives. Worker
/// threads must therefore never fork managed children (tokio's blocking pool retires idle
/// threads on its own schedule); this thread does every managed fork and lives for the
/// process.
#[derive(Debug)]
struct Forker {
    requests: mpsc::Sender<ForkRequest>,
}

impl Forker {
    fn new() -> Self {
        let (requests, queue) = mpsc::channel::<ForkRequest>();
        std::thread::Builder::new()
            .name("spawner-forker".to_string())
            .spawn(move || {
                for mut request in queue {
                    let child = request.command.spawn();
                    let _ = request.reply.send(child);
                }
            })
            .expect("spawning the spawner fork thread");
        Self { requests }
    }

    /// Fork one managed child on the forker thread; blocks until the fork happened (or
    /// failed). The thread's job is ownership of the fork, not throughput.
    fn fork(&self, command: Command) -> io::Result<Child> {
        let (reply, answer) = mpsc::channel();
        self.requests
            .send(ForkRequest { command, reply })
            .map_err(|_| io::Error::other("spawner fork thread is gone"))?;
        answer
            .recv()
            .map_err(|_| io::Error::other("spawner fork thread dropped the reply"))?
    }
}

/// The pre-fork pieces of the parent-death PEP. Everything the child-side closure needs is
/// prepared here — the closure itself runs after `fork`, where allocation and locks are
/// off-limits.
struct ParentDeathPep {
    expected_parent: libc::pid_t,
    /// `<scope>/cgroup.procs` when containment is on: the child writes "0" (itself) there to
    /// join its session scope.
    scope_procs: Option<CString>,
}

/// `cgroup.procs` accepts "0" as "the writing process": the child joins the scope itself.
const PROCS_SELF: &[u8] = b"0";

/// Attach the parent-death PEP to a compositor command. Async-signal-safe syscalls only —
/// this closure runs in the forked child, before exec:
///
/// 1. `PR_SET_PDEATHSIG = SIGKILL`: the manager's death kills the child;
/// 2. a `getppid()` re-check: PDEATHSIG never fires for a parent that is already dead when
///    it is set, so a fork racing the manager's death exits itself (101; `_exit`, not
///    `std::process::exit` — a post-fork child must not run atexit handlers or take locks);
/// 3. with containment: `open(<scope>/cgroup.procs)` + `write("0")` joins the session scope.
///    A failed join fails the spawn: containment fails closed, never silently uncontained.
fn attach_parent_death_pep(command: &mut Command, pep: &ParentDeathPep) {
    let expected_parent = pep.expected_parent;
    let scope_procs = pep.scope_procs.clone();
    // SAFETY: between fork and exec only async-signal-safe calls are made — prctl, getppid,
    // open, write, close, _exit — with no allocation, locking, or Rust-runtime use.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != expected_parent {
                libc::_exit(101);
            }
            if let Some(path) = scope_procs.as_deref() {
                let fd = libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
                if fd < 0 {
                    return Err(io::Error::last_os_error());
                }
                let written = libc::write(fd, PROCS_SELF.as_ptr().cast(), PROCS_SELF.len());
                let error = io::Error::last_os_error();
                libc::close(fd);
                if written != 1 {
                    return Err(error);
                }
            }
            Ok(())
        });
    }
}

/// The production spawner: spawns the chain as managed children (forked on the forker
/// thread), keeps the process table, and tears sessions down to nothing. One instance per
/// manager process.
#[derive(Debug)]
pub struct LinuxSpawner {
    config: SpawnerConfig,
    sessions: Mutex<HashMap<String, ManagedSession>>,
    forker: Forker,
}

impl LinuxSpawner {
    pub fn new(config: SpawnerConfig) -> Self {
        Self {
            config,
            sessions: Mutex::new(HashMap::new()),
            forker: Forker::new(),
        }
    }

    /// Both compositor children log into the session dir; the renderer line the gate reads
    /// lives in Hyprland's instance log.
    fn log_stdio(dir: &Path, name: &str) -> std::io::Result<(Stdio, Stdio)> {
        let file = File::create(dir.join(format!("{name}.log")))?;
        let err = file.try_clone()?;
        Ok((Stdio::from(file), Stdio::from(err)))
    }

    fn wait_for(mut ready: impl FnMut() -> bool, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if ready() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn kill_quietly(child: &mut Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    /// Best-effort scope teardown: `cgroup.kill` everything in the scope (the completion
    /// guarantee for the whole tree), then drop the scope dir. `None` = containment off.
    fn discard_scope(scope: Option<&Path>) {
        if let Some(scope) = scope {
            let _ = kill_scope(scope);
            let _ = fs::remove_dir(scope); // kill_scope removes it; belt for partial states
        }
    }

    /// Best-effort removal of a session's own directories.
    fn discard_dirs(dir: &Path, rt_dir: &Path) {
        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_dir_all(rt_dir);
    }

    /// The compositors' runtime dir: the configured base first, then `/tmp`, then the
    /// manager's `$XDG_RUNTIME_DIR` — the first candidate that keeps the IPC socket under
    /// the unix `sun_path` cap. Refuses only when no candidate fits.
    fn pick_runtime_dir(config: &SpawnerConfig, id: &str) -> Result<PathBuf, ErrorEnvelope> {
        let mut candidates = vec![config.short_root.clone(), PathBuf::from("/tmp")];
        if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
            candidates.push(PathBuf::from(xdg));
        }
        first_fitting_runtime_dir(&candidates, id).ok_or_else(|| {
            ErrorEnvelope::internal(format!(
                "no runtime dir fits the compositor IPC socket budget (tried: {})",
                candidates
                    .iter()
                    .map(|base| base.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
    }

    /// The tail of a session log, for failure paths (the dir is gone right after).
    fn log_tail(dir: &Path, name: &str) -> String {
        match fs::read_to_string(dir.join(name)) {
            Ok(text) => {
                let lines: Vec<&str> = text.lines().collect();
                let start = lines.len().saturating_sub(12);
                format!("; {name} tail: {}", lines[start..].join(" | "))
            }
            Err(_) => format!("; {name} missing"),
        }
    }

    /// The full failure dump for a timed-out child: logs, directory walk, instance log, the
    /// child's kernel state. Failure-path only; the dir is removed right after.
    fn failure_dump(dir: &Path, rt_dir: &Path, child_pid: u32) -> String {
        let mut out = String::new();
        out.push_str(&Self::log_tail(dir, "hyprland.log"));
        out.push_str(&Self::log_tail(dir, "mutter.log"));
        let list = |path: &Path| -> String {
            match fs::read_dir(path) {
                Ok(entries) => entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(" "),
                Err(_) => "(missing)".to_string(),
            }
        };
        out.push_str(&format!("; dir: {}", list(dir)));
        out.push_str(&format!("; rt dir: {}", list(rt_dir)));
        out.push_str(&format!("; hypr dir: {}", list(&rt_dir.join("hypr"))));
        if let Ok(entries) = fs::read_dir(rt_dir.join("hypr")) {
            for entry in entries.flatten() {
                let path = entry.path();
                out.push_str(&format!(
                    "; subdir {}: {}",
                    entry.file_name().to_string_lossy(),
                    list(&path)
                ));
                let log = path.join("hyprland.log");
                if let Ok(text) = fs::read_to_string(&log) {
                    let lines: Vec<&str> = text.lines().collect();
                    let start = lines.len().saturating_sub(18);
                    out.push_str(&format!(
                        "; instance log tail: {}",
                        lines[start..].join(" | ")
                    ));
                }
            }
        }
        if let Ok(output) = Command::new("find")
            .arg(rt_dir)
            .args(["-name", "*.sock", "-o", "-name", ".socket*"])
            .output()
        {
            out.push_str(&format!(
                "; find socks: {}",
                String::from_utf8_lossy(&output.stdout).replace('\n', " ")
            ));
        }
        // Where else could a runtime socket be hiding?
        if let Ok(output) = Command::new("find")
            .args(["/tmp", "/run", "-name", ".socket.sock"])
            .output()
        {
            let found = String::from_utf8_lossy(&output.stdout).replace('\n', " ");
            let found: Vec<&str> = found.split_whitespace().take(5).collect();
            out.push_str(&format!("; find sockets elsewhere: {}", found.join(" ")));
        }
        // Live probe: if hyprctl answers, the IPC socket exists where hyprctl looks.
        let sig = fs::read_dir(rt_dir.join("hypr"))
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .max_by_key(String::len);
        if let Some(sig) = sig
            && let Ok(output) = Command::new("hyprctl")
                .arg("monitors")
                .env("XDG_RUNTIME_DIR", rt_dir)
                .env("HYPRLAND_INSTANCE_SIGNATURE", &sig)
                .stdin(Stdio::null())
                .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            out.push_str(&format!(
                "; hyprctl probe ({sig}): rc={:?} out={} err={}",
                output.status.code(),
                stdout.trim().replace('\n', " "),
                stderr.trim().replace('\n', " ")
            ));
        }
        let state = fs::read_to_string(format!("/proc/{child_pid}/status"))
            .ok()
            .and_then(|status| {
                status
                    .lines()
                    .find(|line| line.starts_with("State:"))
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "(no /proc status)".to_string());
        let wchan = fs::read_to_string(format!("/proc/{child_pid}/wchan")).unwrap_or_default();
        out.push_str(&format!(
            "; child pid {child_pid}: {state} wchan={wchan}; thread: {:?}",
            std::thread::current().name()
        ));
        if let Ok(output) = Command::new("ps").args(["-ef"]).output() {
            let text = String::from_utf8_lossy(&output.stdout);
            let rows: Vec<&str> = text
                .lines()
                .filter(|line| {
                    line.contains("mutter")
                        || line.contains("Hyprland")
                        || line.contains("xkbcomp")
                        || line.contains("Xwayland")
                })
                .collect();
            out.push_str(&format!("; ps: {}", rows.join(" | ")));
        }
        out
    }

    /// `(parent, child)` pids for a live session — used by tests and the health slice.
    pub fn child_pids(&self, id: &str) -> Option<(u32, u32)> {
        let sessions = self.sessions.lock().expect("spawner table");
        sessions.get(id).map(|s| (s.parent.id(), s.child.id()))
    }

    /// One health pass over a session: child-process liveness via a non-blocking wait (a
    /// died-but-unreaped child counts as dead — and gets reaped here, so no zombies), plus a
    /// trivial compositor query. Missing session → `None`.
    pub fn health(&self, id: &str) -> Option<HealthReport> {
        let mut sessions = self.sessions.lock().expect("spawner table");
        let session = sessions.get_mut(id)?;
        let parent_alive = matches!(session.parent.try_wait(), Ok(None));
        let child_alive = matches!(session.child.try_wait(), Ok(None));
        let compositor_answers = child_alive && self.compositor_answers(session);
        Some(HealthReport {
            parent_alive,
            child_alive,
            compositor_answers,
        })
    }

    fn compositor_answers(&self, session: &ManagedSession) -> bool {
        Command::new(&self.config.hyprctl_bin)
            .arg("version")
            .env("XDG_RUNTIME_DIR", &session.rt_dir)
            .env("HYPRLAND_INSTANCE_SIGNATURE", &session.signature)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
}

impl SessionSpawner for LinuxSpawner {
    fn spawn(&self, spec: &SessionSpec) -> Result<String, ErrorEnvelope> {
        // Physical mode: mode = logical × scale (P2 semantics; the constants live here).
        let logical = Resolution::parse(&spec.resolution).map_err(|_| {
            ErrorEnvelope::bad_request(format!("invalid resolution {:?}", spec.resolution))
        })?;
        let mode_w = ((logical.width as f64) * spec.scale).round().max(1.0) as u32;
        let mode_h = ((logical.height as f64) * spec.scale).round().max(1.0) as u32;

        let id = uuid::Uuid::new_v4().to_string();
        let dir = self.config.runtime_root.join(&id);
        // The compositors' XDG_RUNTIME_DIR: short by design — the first fitting candidate of
        // the configured base, /tmp and $XDG_RUNTIME_DIR (see `pick_runtime_dir`).
        let rt_dir = Self::pick_runtime_dir(&self.config, &id)?;
        // Fresh slate: never inherit sockets from a stale crashed session reusing this name.
        let _ = fs::remove_dir_all(&rt_dir);
        fs::create_dir_all(&dir)
            .map_err(|error| ErrorEnvelope::internal(format!("session runtime dir: {error}")))?;
        fs::create_dir_all(&rt_dir)
            .map_err(|error| ErrorEnvelope::internal(format!("compositor runtime dir: {error}")))?;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
        let _ = fs::set_permissions(&rt_dir, fs::Permissions::from_mode(0o700));

        fs::write(
            dir.join("hypr.conf"),
            "debug {\n    disable_logs = false\n}\n",
        )
        .map_err(|error| {
            Self::discard_dirs(&dir, &rt_dir);
            ErrorEnvelope::internal(format!("session config: {error}"))
        })?;

        // Parent: mutter --headless (no KMS, no seat; renders on the render node).
        let mode = format!("{mode_w}x{mode_h}");
        let parent_display = format!("--wayland-display={PARENT_SOCKET}");
        let (out, err) = Self::log_stdio(&dir, "mutter").map_err(|error| {
            Self::discard_dirs(&dir, &rt_dir);
            ErrorEnvelope::internal(format!("session log: {error}"))
        })?;

        // The session scope (containment On) exists before anything is spawned; the
        // compositors self-join it via the PEP. A scope that cannot be created fails the
        // spawn — containment never degrades silently.
        let scope = match &self.config.containment {
            Containment::Off => None,
            Containment::On { subtree } => Some(
                create_session_scope(subtree, scope_name(&id)).map_err(|error| {
                    Self::discard_dirs(&dir, &rt_dir);
                    ErrorEnvelope::internal(format!("session scope: {error}"))
                })?,
            ),
        };
        // Everything the child-side closure needs, prepared before the fork (no allocation
        // and no locks between fork and exec).
        let pep = ParentDeathPep {
            expected_parent: std::process::id() as libc::pid_t,
            scope_procs: scope
                .as_ref()
                .map(|path| CString::new(path.join("cgroup.procs").as_os_str().as_bytes()))
                .transpose()
                .map_err(|error| {
                    Self::discard_scope(scope.as_deref());
                    Self::discard_dirs(&dir, &rt_dir);
                    ErrorEnvelope::internal(format!("session scope procs path: {error}"))
                })?,
        };

        let mut parent_command = Command::new(&self.config.mutter_bin);
        parent_command
            .args([
                "--wayland",
                "--no-x11",
                "--headless",
                "--virtual-monitor",
                mode.as_str(),
                parent_display.as_str(),
            ])
            .env("XDG_RUNTIME_DIR", &rt_dir)
            .env("HOME", &dir)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err);
        attach_parent_death_pep(&mut parent_command, &pep);
        let mut parent = self.forker.fork(parent_command).map_err(|error| {
            Self::discard_scope(scope.as_deref());
            Self::discard_dirs(&dir, &rt_dir);
            ErrorEnvelope::internal(format!("spawning mutter: {error}"))
        })?;

        if !Self::wait_for(
            || rt_dir.join(PARENT_SOCKET).exists(),
            self.config.parent_ready_timeout,
        ) {
            let detail = Self::log_tail(&dir, "mutter.log");
            Self::discard_scope(scope.as_deref());
            Self::kill_quietly(&mut parent);
            Self::discard_dirs(&dir, &rt_dir);
            return Err(ErrorEnvelope::internal(format!(
                "mutter parent did not present its wayland socket in time{detail}"
            )));
        }

        // Child: Hyprland nested on the parent — same PEP and forker: the child carries the
        // same guarantees as the parent. (This failure path runs after the parent exists:
        // kill it and discard the scope, like every other mid-spawn failure.)
        let (out, err) = Self::log_stdio(&dir, "hyprland").map_err(|error| {
            Self::discard_scope(scope.as_deref());
            Self::kill_quietly(&mut parent);
            Self::discard_dirs(&dir, &rt_dir);
            ErrorEnvelope::internal(format!("session log: {error}"))
        })?;
        let mut child_command = Command::new(&self.config.hyprland_bin);
        child_command
            .arg("--i-am-really-stupid")
            .env("WAYLAND_DISPLAY", PARENT_SOCKET)
            .env("HYPRLAND_CONFIG", dir.join("hypr.conf"))
            .env("XDG_RUNTIME_DIR", &rt_dir)
            .env("HOME", &dir)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err);
        attach_parent_death_pep(&mut child_command, &pep);
        let mut child = match self.forker.fork(child_command) {
            Ok(child) => child,
            Err(error) => {
                Self::discard_scope(scope.as_deref());
                Self::kill_quietly(&mut parent);
                Self::discard_dirs(&dir, &rt_dir);
                return Err(ErrorEnvelope::internal(format!(
                    "spawning Hyprland: {error}"
                )));
            }
        };

        let mut signature = String::new();
        let child_ready = Self::wait_for(
            || {
                let Ok(entries) = fs::read_dir(rt_dir.join("hypr")) else {
                    return false;
                };
                for entry in entries.flatten() {
                    let candidate = entry.path();
                    if candidate.join(".socket.sock").exists() {
                        signature = entry.file_name().to_string_lossy().to_string();
                        return true;
                    }
                }
                false
            },
            self.config.child_ready_timeout,
        );
        if !child_ready {
            let detail = Self::failure_dump(&dir, &rt_dir, child.id());
            Self::discard_scope(scope.as_deref());
            Self::kill_quietly(&mut child);
            Self::kill_quietly(&mut parent);
            Self::discard_dirs(&dir, &rt_dir);
            return Err(ErrorEnvelope::internal(format!(
                "Hyprland child did not present its IPC socket in time{detail}"
            )));
        }

        // The output at the requested mode and scale (P2 creation params).
        let scale = format!("{}", spec.scale);
        let monitor_rule = format!("{PRIMARY_OUTPUT},{mode_w}x{mode_h}@60,0x0,{scale}");
        let run_hyprctl = |args: &[&str]| -> Result<(), String> {
            match Command::new(&self.config.hyprctl_bin)
                .args(args)
                .env("XDG_RUNTIME_DIR", &rt_dir)
                .env("HYPRLAND_INSTANCE_SIGNATURE", &signature)
                .stdin(Stdio::null())
                .output()
            {
                Ok(output) if output.status.success() => Ok(()),
                Ok(output) => Err(String::from_utf8_lossy(&output.stderr).trim().to_string()),
                Err(error) => Err(error.to_string()),
            }
        };
        let mut created = run_hyprctl(&["output", "create", "headless", PRIMARY_OUTPUT]);
        if created.is_ok() {
            created = run_hyprctl(&["keyword", "monitor", &monitor_rule]);
        }
        if let Err(detail) = created {
            Self::discard_scope(scope.as_deref());
            Self::kill_quietly(&mut child);
            Self::kill_quietly(&mut parent);
            Self::discard_dirs(&dir, &rt_dir);
            return Err(ErrorEnvelope::internal(format!(
                "hyprctl output setup failed: {detail}"
            )));
        }

        self.sessions.lock().expect("spawner table").insert(
            id.clone(),
            ManagedSession {
                dir,
                rt_dir,
                scope,
                parent,
                child,
                signature,
            },
        );
        Ok(id)
    }

    fn kill_tree(&self, session_id: &str) -> Result<(), ErrorEnvelope> {
        let Some(mut session) = self
            .sessions
            .lock()
            .expect("spawner table")
            .remove(session_id)
        else {
            return Err(ErrorEnvelope::session_not_found());
        };
        // Scope first: `cgroup.kill` is the completion guarantee for the whole tree —
        // grandchildren and browser process trees included. Then our direct children (child
        // first: it dies with the parent anyway, but be explicit), then the dirs.
        Self::discard_scope(session.scope.as_deref());
        Self::kill_quietly(&mut session.child);
        Self::kill_quietly(&mut session.parent);
        Self::discard_dirs(&session.dir, &session.rt_dir);
        Ok(())
    }
}

impl Drop for LinuxSpawner {
    fn drop(&mut self) {
        let mut sessions = self.sessions.lock().expect("spawner table");
        for (_, mut session) in sessions.drain() {
            Self::discard_scope(session.scope.as_deref());
            Self::kill_quietly(&mut session.child);
            Self::kill_quietly(&mut session.parent);
            Self::discard_dirs(&session.dir, &session.rt_dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MUTTER_STUB: &str = "#!/bin/sh\nfor a in \"$@\"; do\n  case \"$a\" in\n    --wayland-display=*) touch \"$XDG_RUNTIME_DIR/${a#*=}\" ;;\n  esac\ndone\nwhile :; do sleep 1; done\n";
    const HYPR_STUB: &str = "#!/bin/sh\nmkdir -p \"$XDG_RUNTIME_DIR/hypr/test-signature\"\ntouch \"$XDG_RUNTIME_DIR/hypr/test-signature/.socket.sock\"\nwhile :; do sleep 1; done\n";
    const HYPRCTL_STUB: &str = "#!/bin/sh\necho ok\nexit 0\n";
    const IDLE_STUB: &str = "#!/bin/sh\nwhile :; do sleep 1; done\n";

    fn write_stub(dir: &Path, name: &str, body: &str) -> String {
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    fn stub_config(stub_dir: &Path, runtime_root: &Path) -> SpawnerConfig {
        let mut config = SpawnerConfig::new(runtime_root);
        config.mutter_bin = write_stub(stub_dir, "mutter", MUTTER_STUB);
        config.hyprland_bin = write_stub(stub_dir, "Hyprland", HYPR_STUB);
        config.hyprctl_bin = write_stub(stub_dir, "hyprctl", HYPRCTL_STUB);
        config.parent_ready_timeout = Duration::from_secs(3);
        config.child_ready_timeout = Duration::from_secs(3);
        // Never inherit an ambient TMPDIR here: the runtime-dir budget is a unit under test
        // (`runtime_dir_picker_*`), not something the box's environment may decide.
        config.short_root = PathBuf::from("/tmp");
        config
    }

    fn proc_alive(pid: u32) -> bool {
        Path::new(&format!("/proc/{pid}")).exists()
    }

    fn stub_processes_running(stub_path: &str) -> bool {
        let Ok(entries) = fs::read_dir("/proc") else {
            return false;
        };
        for entry in entries.flatten() {
            let cmdline = entry.path().join("cmdline");
            if let Ok(bytes) = fs::read(cmdline)
                && String::from_utf8_lossy(&bytes).contains(stub_path)
            {
                return true;
            }
        }
        false
    }

    #[test]
    fn spawn_readiness_and_teardown_with_stubs() {
        let tmp = tempfile::tempdir().unwrap();
        let stubs = tmp.path().join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        let runtime = tmp.path().join("run");
        let config = stub_config(&stubs, &runtime);
        let mutter_path = config.mutter_bin.clone();
        let spawner = LinuxSpawner::new(config);

        let spec = SessionSpec {
            backend: crate::manager::Backend::Headless,
            resolution: "1920x1080".to_string(),
            scale: 2.0,
            view: false,
            label: None,
        };
        let id = spawner.spawn(&spec).expect("spawn succeeds");
        let (parent_pid, child_pid) = spawner.child_pids(&id).expect("session tracked");
        assert!(proc_alive(parent_pid), "parent alive after spawn");
        assert!(proc_alive(child_pid), "child alive after spawn");
        let dir = runtime.join(&id);
        let rt_dir = PathBuf::from("/tmp").join(format!("dr-{}", &id[..8]));
        assert!(rt_dir.join("wayland-m").exists(), "parent socket present");
        assert!(rt_dir.join("hypr/test-signature/.socket.sock").exists());
        assert!(dir.join("mutter.log").exists() && dir.join("hyprland.log").exists());

        spawner.kill_tree(&id).expect("kill_tree succeeds");
        assert!(!proc_alive(parent_pid), "parent gone after teardown");
        assert!(!proc_alive(child_pid), "child gone after teardown");
        assert!(!dir.exists(), "runtime dir removed");
        assert!(!rt_dir.exists(), "compositor runtime dir removed");
        assert!(!stub_processes_running(&mutter_path), "no stub left behind");
    }

    #[test]
    fn parent_timeout_is_an_error_and_leaves_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let stubs = tmp.path().join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        let runtime = tmp.path().join("run");
        let mut config = stub_config(&stubs, &runtime);
        let slow = write_stub(&stubs, "mutter-slow", IDLE_STUB);
        config.mutter_bin = slow.clone();
        let spawner = LinuxSpawner::new(config);

        let spec = SessionSpec::default();
        let error = spawner.spawn(&spec).unwrap_err();
        assert!(
            error.message.contains("did not present its wayland socket"),
            "got: {error:?}"
        );
        assert!(!stub_processes_running(&slow), "slow parent was killed");
        if let Ok(entries) = fs::read_dir(&runtime) {
            assert_eq!(entries.count(), 0, "no session dir left behind");
        }
    }

    #[test]
    fn child_timeout_kills_the_parent_too() {
        let tmp = tempfile::tempdir().unwrap();
        let stubs = tmp.path().join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        let runtime = tmp.path().join("run");
        let mut config = stub_config(&stubs, &runtime);
        config.hyprland_bin = write_stub(&stubs, "Hyprland-slow", IDLE_STUB);
        let mutter_path = config.mutter_bin.clone();
        let spawner = LinuxSpawner::new(config);

        let spec = SessionSpec::default();
        let error = spawner.spawn(&spec).unwrap_err();
        assert!(
            error.message.contains("IPC socket in time"),
            "got: {error:?}"
        );
        assert!(
            !stub_processes_running(&mutter_path),
            "parent killed after child failure"
        );
    }

    #[test]
    fn kill_tree_unknown_session_is_session_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let stubs = tmp.path().join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        let spawner = LinuxSpawner::new(stub_config(&stubs, &tmp.path().join("run")));
        let error = spawner.kill_tree("nope").unwrap_err();
        assert_eq!(error.code, crate::ipc::ErrorCode::SessionNotFound);
    }

    /// hyprctl that stands in for a compositor that stopped answering: setup calls succeed,
    /// the health query fails.
    const HYPRCTL_FLAP_STUB: &str =
        "#!/bin/sh\ncase \"$1\" in\n  version) exit 1 ;;\nesac\nexit 0\n";

    fn kill9(pid: u32) {
        let status = Command::new("sh")
            .args(["-c", &format!("kill -9 {pid}")])
            .status()
            .expect("sh runs");
        assert!(status.success(), "kill -9 {pid}");
        // Give the kernel a moment to make the death observable to try_wait.
        std::thread::sleep(Duration::from_millis(200));
    }

    #[test]
    fn health_walks_running_to_dead_when_the_child_dies() {
        let tmp = tempfile::tempdir().unwrap();
        let stubs = tmp.path().join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        let runtime = tmp.path().join("run");
        let spawner = LinuxSpawner::new(stub_config(&stubs, &runtime));

        let id = spawner.spawn(&SessionSpec::default()).expect("spawn");
        let report = spawner.health(&id).expect("session tracked");
        assert_eq!(report.state(), SessionState::Running, "{report:?}");

        let (_, child_pid) = spawner.child_pids(&id).unwrap();
        kill9(child_pid);
        let report = spawner.health(&id).expect("session tracked");
        assert!(!report.child_alive, "{report:?}");
        assert_eq!(report.state(), SessionState::Dead, "{report:?}");

        // Teardown of a dead session still works (idempotent kill + cleanup).
        spawner
            .kill_tree(&id)
            .expect("teardown of a dead session still works");
    }

    #[test]
    fn health_reports_degraded_when_the_compositor_goes_silent() {
        let tmp = tempfile::tempdir().unwrap();
        let stubs = tmp.path().join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        let runtime = tmp.path().join("run");
        let mut config = stub_config(&stubs, &runtime);
        config.hyprctl_bin = write_stub(&stubs, "hyprctl-flap", HYPRCTL_FLAP_STUB);
        let spawner = LinuxSpawner::new(config);

        let id = spawner.spawn(&SessionSpec::default()).expect("spawn");
        let report = spawner.health(&id).expect("session tracked");
        assert!(report.parent_alive && report.child_alive, "{report:?}");
        assert!(!report.compositor_answers, "{report:?}");
        assert_eq!(report.state(), SessionState::Degraded, "{report:?}");
        spawner.kill_tree(&id).expect("teardown");
    }

    #[test]
    fn health_unknown_session_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let stubs = tmp.path().join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        let spawner = LinuxSpawner::new(stub_config(&stubs, &tmp.path().join("run")));
        assert!(spawner.health("nope").is_none());
    }

    #[test]
    fn runtime_dir_picker_skips_deep_candidates_and_refuses_when_none_fit() {
        let deep = PathBuf::from("/some/very/deep/place").join("a".repeat(40));
        let short = PathBuf::from("/tmp");
        let id = "2f7a4c1e-9b3d-4a6f-8e21-77aa11bb22cc";

        let picked = first_fitting_runtime_dir(&[deep.clone(), short.clone()], id)
            .expect("the short candidate fits");
        assert_eq!(picked, short.join("dr-2f7a4c1e"));

        assert!(
            first_fitting_runtime_dir(&[deep.clone(), deep.join("deeper")], id).is_none(),
            "deep-only candidate lists refuse instead of overflowing silently"
        );
    }

    #[test]
    fn spawn_falls_back_to_a_short_base_when_the_configured_one_is_deep() {
        let tmp = tempfile::tempdir().unwrap();
        let stubs = tmp.path().join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        let runtime = tmp.path().join("run");
        let mut config = stub_config(&stubs, &runtime);
        config.short_root = tmp
            .path()
            .join("deep/enough/to/overflow/the/socket/budget/for/sure");
        let spawner = LinuxSpawner::new(config);

        let id = spawner
            .spawn(&SessionSpec::default())
            .expect("a deep configured base falls back instead of refusing");
        let rt_dir = PathBuf::from("/tmp").join(format!("dr-{}", &id[..8]));
        assert!(rt_dir.exists(), "fell back to /tmp: {}", rt_dir.display());

        spawner.kill_tree(&id).expect("teardown");
        assert!(!rt_dir.exists(), "fallback runtime dir removed on teardown");
    }

    #[test]
    fn containment_defaults_to_off_for_existing_callers() {
        // Off is the default: a plain `SpawnerConfig::new` keeps today's behavior; turning
        // containment on is an explicit act.
        assert_eq!(Containment::default(), Containment::Off);
        assert_eq!(
            SpawnerConfig::new("/run/daiorama").containment,
            Containment::Off
        );
        let mut config = SpawnerConfig::new("/run/daiorama");
        config.containment = Containment::On {
            subtree: PathBuf::from("/cg/daiorama-sessions"),
        };
        assert_eq!(
            session_scope_path(&config.containment, "2f7a4c1e-9b3d-4a6f-8e21-77aa11bb22cc"),
            Some(PathBuf::from("/cg/daiorama-sessions/2f7a4c1e"))
        );
    }

    #[test]
    fn scope_path_selection_follows_containment_and_the_id() {
        let id = "2f7a4c1e-9b3d-4a6f-8e21-77aa11bb22cc";
        assert_eq!(
            session_scope_path(&Containment::Off, id),
            None,
            "off computes no scope path"
        );
        let mode = Containment::On {
            subtree: PathBuf::from("/cg/manager/daiorama-sessions"),
        };
        assert_eq!(
            session_scope_path(&mode, id),
            Some(PathBuf::from("/cg/manager/daiorama-sessions/2f7a4c1e"))
        );
        // Short ids stay whole — the helper is total, nothing indexes out of range.
        assert_eq!(scope_name("abc"), "abc");
        assert_eq!(
            session_scope_path(&mode, "abc"),
            Some(PathBuf::from("/cg/manager/daiorama-sessions/abc"))
        );

        // What the spawner creates is the path this rule names: on a plain directory (the
        // real-cgroupfs checks are the ignored real_box tests), scope creation lands exactly
        // on the selected path and writes the limit files there.
        let fake_subtree = tempfile::tempdir().unwrap();
        let created = create_session_scope(fake_subtree.path(), scope_name(id)).unwrap();
        let expected = session_scope_path(
            &Containment::On {
                subtree: fake_subtree.path().to_path_buf(),
            },
            id,
        )
        .unwrap();
        assert_eq!(
            created, expected,
            "scope creation lands on the selected path"
        );
        assert!(created.join("memory.max").exists(), "limit file written");
    }

    #[test]
    fn containment_on_with_an_absent_subtree_fails_the_spawn_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let stubs = tmp.path().join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        let runtime = tmp.path().join("run");
        let mut config = stub_config(&stubs, &runtime);
        let mutter_path = config.mutter_bin.clone();
        config.containment = Containment::On {
            subtree: tmp.path().join("absent-cgroupfs"),
        };
        let spawner = LinuxSpawner::new(config);

        let error = spawner.spawn(&SessionSpec::default()).unwrap_err();
        assert!(error.message.contains("session scope"), "got: {error:?}");
        if let Ok(entries) = fs::read_dir(&runtime) {
            assert_eq!(entries.count(), 0, "no session dir left behind");
        }
        assert!(!stub_processes_running(&mutter_path), "nothing was spawned");
    }

    #[test]
    fn pep_fails_the_spawn_when_the_scope_join_is_impossible() {
        // The scope self-join is part of the spawn: if the child cannot join, the spawn
        // fails — containment never degrades silently.
        let forker = Forker::new();
        let pep = ParentDeathPep {
            expected_parent: std::process::id() as libc::pid_t,
            scope_procs: Some(CString::new("/nonexistent-daiorama/cgroup.procs").unwrap()),
        };
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("exit 0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        attach_parent_death_pep(&mut command, &pep);

        let error = forker
            .fork(command)
            .expect_err("a join that cannot happen fails the spawn");
        assert!(error.to_string().contains("No such file"), "got: {error}");
    }

    #[test]
    fn forker_forks_on_its_own_named_thread() {
        let forker = Forker::new();
        // Serve a round trip first: once a request has been answered, the thread is
        // provably running, under its own name (the name is set before the loop runs).
        let mut command = Command::new("sh");
        command
            .args(["-c", "exit 7"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = forker.fork(command).expect("fork round-trips");
        assert_eq!(child.wait().expect("wait").code(), Some(7), "the child ran");

        // The parent-death binding is per-thread: the forker must exist as its own thread.
        let named = fs::read_dir("/proc/self/task")
            .expect("/proc/self/task readable")
            .flatten()
            .any(|entry| {
                fs::read_to_string(entry.path().join("comm"))
                    .map(|comm| comm.trim() == "spawner-forker")
                    .unwrap_or(false)
            });
        assert!(named, "the forker thread is alive under its own name");
    }
}
