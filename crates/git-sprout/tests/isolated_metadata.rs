// ABOUTME: Proves isolated Sprout workspaces keep writable Git metadata out of the source.
// ABOUTME: Exercises config and submodule writes while the source repository is read-only.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn git(directory: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .expect("run git")
}

fn assert_git(directory: &Path, args: &[&str]) {
    let output = git(directory, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn set_read_only_tree(path: &Path, read_only: bool) {
    if path.is_dir() {
        for entry in fs::read_dir(path).expect("read tree") {
            set_read_only_tree(&entry.expect("tree entry").path(), read_only);
        }
    }
    let mode = if path.is_dir() {
        if read_only {
            0o555
        } else {
            0o755
        }
    } else if read_only {
        0o444
    } else {
        0o644
    };
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("set permissions");
}

#[test]
fn isolated_metadata_supports_private_config_and_submodules_from_read_only_source() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("git-sprout-isolated-{nonce}"));
    let child = root.join("child");
    let source = root.join("source");
    let destination = root.join("destination");
    fs::create_dir_all(&child).expect("create child");
    fs::create_dir_all(&source).expect("create source");

    for repository in [&child, &source] {
        assert_git(repository, &["init", "-q"]);
        assert_git(repository, &["config", "user.name", "Sprout Test"]);
        assert_git(
            repository,
            &["config", "user.email", "sprout@example.invalid"],
        );
    }

    fs::write(child.join("payload.txt"), "child\n").expect("write child");
    assert_git(&child, &["add", "payload.txt"]);
    assert_git(&child, &["commit", "-qm", "child"]);

    fs::write(source.join("root.txt"), "root\n").expect("write source");
    assert_git(&source, &["add", "root.txt"]);
    assert_git(&source, &["commit", "-qm", "root"]);
    assert_git(
        &source,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            child.to_str().expect("utf8 child path"),
            "dependency",
        ],
    );
    assert_git(&source, &["commit", "-qam", "submodule"]);

    let source_config = fs::read(source.join(".git/config")).expect("read source config");
    set_read_only_tree(&source, true);

    let output = Command::new(env!("CARGO_BIN_EXE_git-sprout"))
        .current_dir(&source)
        .args([
            "add",
            "--isolated-metadata",
            "--detach",
            destination.to_str().expect("utf8 destination path"),
            "HEAD",
        ])
        .output()
        .expect("run git-sprout");
    assert!(
        output.status.success(),
        "git-sprout failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(destination.join(".git").is_dir());
    assert_git(&destination, &["config", "isolated.test", "yes"]);
    assert_git(
        &destination,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "update",
            "--init",
        ],
    );
    assert!(destination.join("dependency/payload.txt").is_file());
    assert_eq!(
        fs::read(source.join(".git/config")).expect("reread source config"),
        source_config
    );

    set_read_only_tree(&source, false);
    let _ = fs::remove_dir_all(root);
}
