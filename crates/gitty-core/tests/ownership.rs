//! A repository owned by another account: gitty must report it as untrusted
//! (never "not a repository"), and trusting it must open it.
//!
//! Its own test binary, with a single test, because it sets process-wide
//! environment variables.

use std::process::Command;

use gitty_core::{Error, Repo};

#[test]
fn untrusted_repo_is_reported_and_can_be_trusted() {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("game");
    std::fs::create_dir_all(repo.join("Assets")).unwrap();
    assert!(Command::new("git").current_dir(&repo).args(["init", "-q"]).status().unwrap().success());
    // a private global config, so the real ~/.gitconfig is never touched
    let global = d.path().join("gitconfig");
    std::fs::write(&global, "").unwrap();
    // SAFETY: this test binary runs only this test, on one thread
    unsafe {
        std::env::set_var("GIT_CONFIG_GLOBAL", &global);
        std::env::set_var("GIT_TEST_ASSUME_DIFFERENT_OWNER", "1");
    }
    // a plain folder is "not a repository", and only that
    let plain = d.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert!(matches!(Repo::discover(&plain), Err(Error::NotARepo(_))));

    let probe = Command::new("git").current_dir(&repo).args(["rev-parse", "--git-dir"]).output().unwrap();
    if !String::from_utf8_lossy(&probe.stderr).contains("dubious ownership") {
        eprintln!("this git build can't simulate another owner; skipping");
        return;
    }

    // from a subfolder too: the path to trust is the repository root
    let Err(err) = Repo::discover(&repo.join("Assets")) else { panic!("untrusted repo must not open") };
    let Error::UntrustedRepo { path } = err else { panic!("expected UntrustedRepo, got {err:?}") };
    let canon = |p: &std::path::Path| p.canonicalize().unwrap();
    assert_eq!(canon(std::path::Path::new(&path)), canon(&repo));

    assert!(gitty_core::repo::trust_folder("*").is_err(), "never trust everything");
    gitty_core::repo::trust_folder(&path).unwrap();
    let cfg = std::fs::read_to_string(&global).unwrap();
    assert!(cfg.contains("[safe]") && cfg.contains("directory = "), "{cfg}");
    let opened = Repo::discover(&repo.join("Assets")).expect("trusted repo opens");
    assert_eq!(canon(opened.root()), canon(&repo));
}
