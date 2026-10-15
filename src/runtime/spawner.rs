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
//! Health checks and the containment scope attach in later slices — through this same table.

use std::collections::HashMap;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::ipc::ErrorEnvelope;
use crate::manager::{Resolution, SessionSpawner, SessionSpec};

/// The parent's wayland socket name inside each session's runtime dir.
pub const PARENT_SOCKET: &str = "wayland-m";

/// The output the spawner creates on the child compositor.
pub const PRIMARY_OUTPUT: &str = "HEADLESS-1";

/// Binaries and paths the spawner drives. Tests point these at stubs.
#[derive(Debug, Clone)]
pub struct SpawnerConfig {
    pub mutter_bin: String,
    pub hyprland_bin: String,
    pub hyprctl_bin: String,
    pub runtime_root: PathBuf,
    pub parent_ready_timeout: Duration,
    pub child_ready_timeout: Duration,
}

impl SpawnerConfig {
    pub fn new(runtime_root: impl Into<PathBuf>) -> Self {
        Self {
            mutter_bin: "mutter".to_string(),
            hyprland_bin: "Hyprland".to_string(),
            hyprctl_bin: "hyprctl".to_string(),
            runtime_root: runtime_root.into(),
            parent_ready_timeout: Duration::from_secs(15),
            child_ready_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug)]
struct ManagedSession {
    dir: PathBuf,
    parent: Child,
    child: Child,
    #[allow(dead_code)] // carried for the health slice (hyprctl needs the signature)
    signature: String,
}

/// The production spawner: spawns the chain as direct children, keeps the process table, and
/// tears sessions down to nothing. One instance per manager process.
#[derive(Debug)]
pub struct LinuxSpawner {
    config: SpawnerConfig,
    sessions: Mutex<HashMap<String, ManagedSession>>,
}

impl LinuxSpawner {
    pub fn new(config: SpawnerConfig) -> Self {
        Self {
            config,
            sessions: Mutex::new(HashMap::new()),
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

    /// `(parent, child)` pids for a live session — used by tests and the health slice.
    pub fn child_pids(&self, id: &str) -> Option<(u32, u32)> {
        let sessions = self.sessions.lock().expect("spawner table");
        sessions.get(id).map(|s| (s.parent.id(), s.child.id()))
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
        fs::create_dir_all(&dir)
            .map_err(|error| ErrorEnvelope::internal(format!("session runtime dir: {error}")))?;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));

        fs::write(
            dir.join("hypr.conf"),
            "debug {\n    disable_logs = false\n}\n",
        )
        .map_err(|error| ErrorEnvelope::internal(format!("session config: {error}")))?;

        // Parent: mutter --headless (no KMS, no seat; renders on the render node).
        let mode = format!("{mode_w}x{mode_h}");
        let parent_display = format!("--wayland-display={PARENT_SOCKET}");
        let (out, err) = Self::log_stdio(&dir, "mutter")
            .map_err(|error| ErrorEnvelope::internal(format!("session log: {error}")))?;
        let mut parent = Command::new(&self.config.mutter_bin)
            .args([
                "--wayland",
                "--no-x11",
                "--headless",
                "--virtual-monitor",
                mode.as_str(),
                parent_display.as_str(),
            ])
            .env("XDG_RUNTIME_DIR", &dir)
            .env("HOME", &dir)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .spawn()
            .map_err(|error| {
                let _ = fs::remove_dir_all(&dir);
                ErrorEnvelope::internal(format!("spawning mutter: {error}"))
            })?;

        if !Self::wait_for(
            || dir.join(PARENT_SOCKET).exists(),
            self.config.parent_ready_timeout,
        ) {
            Self::kill_quietly(&mut parent);
            let _ = fs::remove_dir_all(&dir);
            return Err(ErrorEnvelope::internal(
                "mutter parent did not present its wayland socket in time",
            ));
        }

        // Child: Hyprland nested on the parent.
        let (out, err) = Self::log_stdio(&dir, "hyprland")
            .map_err(|error| ErrorEnvelope::internal(format!("session log: {error}")))?;
        let mut child = match Command::new(&self.config.hyprland_bin)
            .arg("--i-am-really-stupid")
            .env("WAYLAND_DISPLAY", PARENT_SOCKET)
            .env("HYPRLAND_CONFIG", dir.join("hypr.conf"))
            .env("XDG_RUNTIME_DIR", &dir)
            .env("HOME", &dir)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                Self::kill_quietly(&mut parent);
                let _ = fs::remove_dir_all(&dir);
                return Err(ErrorEnvelope::internal(format!(
                    "spawning Hyprland: {error}"
                )));
            }
        };

        let mut signature = String::new();
        let child_ready = Self::wait_for(
            || {
                let Ok(entries) = fs::read_dir(dir.join("hypr")) else {
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
            Self::kill_quietly(&mut child);
            Self::kill_quietly(&mut parent);
            let _ = fs::remove_dir_all(&dir);
            return Err(ErrorEnvelope::internal(
                "Hyprland child did not present its IPC socket in time",
            ));
        }

        // The output at the requested mode and scale (P2 creation params).
        let scale = format!("{}", spec.scale);
        let monitor_rule = format!("{PRIMARY_OUTPUT},{mode_w}x{mode_h}@60,0x0,{scale}");
        let run_hyprctl = |args: &[&str]| -> Result<(), String> {
            match Command::new(&self.config.hyprctl_bin)
                .args(args)
                .env("XDG_RUNTIME_DIR", &dir)
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
            Self::kill_quietly(&mut child);
            Self::kill_quietly(&mut parent);
            let _ = fs::remove_dir_all(&dir);
            return Err(ErrorEnvelope::internal(format!(
                "hyprctl output setup failed: {detail}"
            )));
        }

        self.sessions.lock().expect("spawner table").insert(
            id.clone(),
            ManagedSession {
                dir,
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
        // Child first (it dies with the parent anyway, but be explicit), then the parent.
        Self::kill_quietly(&mut session.child);
        Self::kill_quietly(&mut session.parent);
        let _ = fs::remove_dir_all(&session.dir);
        Ok(())
    }
}

impl Drop for LinuxSpawner {
    fn drop(&mut self) {
        let mut sessions = self.sessions.lock().expect("spawner table");
        for (_, mut session) in sessions.drain() {
            Self::kill_quietly(&mut session.child);
            Self::kill_quietly(&mut session.parent);
            let _ = fs::remove_dir_all(&session.dir);
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
        assert!(dir.join("wayland-m").exists(), "parent socket present");
        assert!(dir.join("hypr/test-signature/.socket.sock").exists());
        assert!(dir.join("mutter.log").exists() && dir.join("hyprland.log").exists());

        spawner.kill_tree(&id).expect("kill_tree succeeds");
        assert!(!proc_alive(parent_pid), "parent gone after teardown");
        assert!(!proc_alive(child_pid), "child gone after teardown");
        assert!(!dir.exists(), "runtime dir removed");
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
}
