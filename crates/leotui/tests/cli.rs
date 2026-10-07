//! The command line, run as a user runs it.

use std::process::Command;

#[test]
fn version_prints_the_package_version_and_exits() {
    for flag in ["--version", "-V"] {
        let out = leotui().arg(flag).output().expect("could not run leotui");
        assert!(out.status.success(), "{flag}: {:?}", out.status);
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(stdout, format!("leotui {}\n", env!("CARGO_PKG_VERSION")));
    }
}

#[test]
fn help_goes_to_stdout_and_a_bad_argument_exits_2() {
    let run = |args: &[&str]| leotui().args(args).output().expect("could not run leotui");
    let help = run(&["--help"]);
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    for flag in ["--dump", "--keys", "--press", "--no-external", "--theme"] {
        assert!(text.contains(flag), "{flag} missing from --help:\n{text}");
    }
    for bad in [&["--bogus"][..], &["a.leo", "b.leo"], &["--width", "x"]] {
        assert_eq!(run(bad).status.code(), Some(2), "{bad:?}");
    }
}

#[test]
fn a_path_with_no_file_starts_a_new_outline_there() {
    let dir = std::env::temp_dir().join(format!("leotui-new-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let leo = dir.join("new.leo");
    let out = leotui()
        .arg(&leo)
        .args(["--dump", "--press", "Ctrl-s"])
        .output()
        .expect("could not run leotui");
    let saved = leo.exists();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(saved, "{}", String::from_utf8_lossy(&out.stdout));
}

/// leotui with no settings: the user's are neither read nor moved.
fn leotui() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_leotui"));
    let none = std::env::temp_dir().join(format!("leotui-no-config-{}", std::process::id()));
    c.env("XDG_CONFIG_HOME", none);
    c
}
