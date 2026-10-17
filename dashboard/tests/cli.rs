//! The module's own CLI, against the real binary: 04 grammar (`connect <address>`, `token
//! set|remove <connection>`) and the 0600 store. The core wrapper execs these verbs, so the
//! grammar here is the contract.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_daiorama-dashboard"))
}

fn run(config: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut command = Command::new(binary());
    command
        .args(args)
        .args(["--config", config.to_str().unwrap()]);
    command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("module runs");
    if let Some(text) = stdin {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    child.wait_with_output().expect("module finishes")
}

fn store(config: &Path) -> String {
    std::fs::read_to_string(config).unwrap_or_default()
}

#[test]
fn connect_takes_the_address_as_its_default_label() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("connections.ron");
    let out = run(&config, &["connect", "127.0.0.1:8756"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = store(&config);
    assert!(text.contains("127.0.0.1:8756"), "{text}");
    assert!(
        !text.contains('<'),
        "no placeholder token in the store: {text}"
    );

    let out = run(&config, &["connect"], None);
    assert!(!out.status.success(), "a bare connect must refuse");
}

#[test]
fn tokens_are_set_from_stdin_never_printed_and_removable() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("connections.ron");
    assert!(
        run(&config, &["connect", "127.0.0.1:8756"], None)
            .status
            .success()
    );

    let out = run(
        &config,
        &["token", "set", "127.0.0.1:8756"],
        Some("instance-secret\n"),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed = String::from_utf8_lossy(&out.stdout);
    assert!(
        !printed.contains("instance-secret"),
        "value must not be printed: {printed}"
    );
    assert!(
        store(&config).contains("instance-secret"),
        "{}",
        store(&config)
    );

    let out = run(
        &config,
        &["scope-token", "set", "127.0.0.1:8756"],
        Some("scope-secret\n"),
    );
    assert!(out.status.success());
    assert!(store(&config).contains("scope-secret"));

    let out = run(&config, &["token", "remove", "127.0.0.1:8756"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !store(&config).contains("instance-secret"),
        "{}",
        store(&config)
    );
    assert!(
        store(&config).contains("scope-secret"),
        "scope secret untouched"
    );
}
