//! The 04 `dashboard` verbs against the real binary: the core CLI locates the module through
//! the activation link (D1) and execs it with the module's own grammar; a missing or skewed
//! module is refused before anything runs.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_daiorama"))
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run(instance: &str, args: &[&str], stdin: Option<&str>) -> Output {
    let tmp = root().join("target").join("dashboard-test");
    let conf = tmp.join("conf");
    let data = tmp.join("data");
    let run_dir = tmp.join("run");
    for dir in [&conf, &data, &run_dir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let mut command = Command::new(binary());
    command
        .env("XDG_CONFIG_HOME", &conf)
        .env("XDG_DATA_HOME", &data)
        .env("XDG_RUNTIME_DIR", &run_dir)
        .args(args)
        .args(["--instance", instance]);
    command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("binary runs");
    if let Some(text) = stdin {
        child
            .stdin
            .as_mut()
            .expect("stdin piped")
            .write_all(text.as_bytes())
            .unwrap();
    }
    child.wait_with_output().expect("binary finishes")
}

fn instance_data(instance: &str) -> PathBuf {
    root()
        .join("target")
        .join("dashboard-test")
        .join("data")
        .join("daiorama")
        .join(instance)
}

fn install(instance: &str, version: &str, body: &str) {
    let dir = instance_data(instance)
        .join("modules")
        .join("daiorama-dashboard")
        .join(version);
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("daiorama-dashboard");
    std::fs::write(&script, body).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let parent = instance_data(instance)
        .join("modules")
        .join("daiorama-dashboard");
    let link = parent.join("active");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(version, &link).unwrap();
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn a_missing_module_is_the_pinned_refusal() {
    let out = run("dash-missing", &["dashboard", "serve"], None);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("module not installed; run 'daiorama dashboard install'."),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_skewed_module_is_refused_before_exec() {
    let instance = "dash-skew";
    let marker = root().join("target").join("dash-skew-ran");
    let _ = std::fs::remove_file(&marker);
    install(
        instance,
        "9.9.9",
        &format!("#!/bin/sh\ntouch {}\n", marker.display()),
    );
    let out = run(instance, &["dashboard", "serve"], None);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("does not match this dashboard"),
        "{}",
        stderr(&out)
    );
    assert!(!marker.exists(), "skewed module must not execute");
}

#[test]
fn verbs_forward_to_the_module_with_its_own_grammar() {
    let instance = "dash-forward";
    install(
        instance,
        env!("CARGO_PKG_VERSION"),
        "#!/bin/sh\necho \"FAKE:$@\"\ncat\n",
    );

    let out = run(instance, &["dashboard", "connect", "127.0.0.1:8756"], None);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("FAKE:connect 127.0.0.1:8756"),
        "{}",
        stdout(&out)
    );

    let out = run(instance, &["dashboard", "connections", "list"], None);
    assert!(
        stdout(&out).contains("FAKE:connections"),
        "{}",
        stdout(&out)
    );

    let out = run(
        instance,
        &["dashboard", "token", "set", "127.0.0.1:8756"],
        Some("s3cret\n"),
    );
    assert!(
        stdout(&out).contains("FAKE:token set 127.0.0.1:8756"),
        "{}",
        stdout(&out)
    );
    assert!(
        stdout(&out).contains("s3cret"),
        "stdin must reach the module: {}",
        stdout(&out)
    );
}

#[test]
fn install_still_refuses_until_packaging_lands_it() {
    let out = run("dash-install", &["dashboard", "install"], None);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("not implemented yet"),
        "{}",
        stderr(&out)
    );
}

#[allow(dead_code)]
fn _path(_: &Path) {}
