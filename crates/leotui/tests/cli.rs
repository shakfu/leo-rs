//! The command line, run as a user runs it.

use std::process::Command;

#[test]
fn version_prints_the_package_version_and_exits() {
    for flag in ["--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_leotui"))
            .arg(flag)
            .output()
            .expect("could not run leotui");
        assert!(out.status.success(), "{flag}: {:?}", out.status);
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(stdout, format!("leotui {}\n", env!("CARGO_PKG_VERSION")));
    }
}
