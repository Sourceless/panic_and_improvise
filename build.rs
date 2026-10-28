// What a build that is not a release calls itself: `dev` and the commit it was made from (`dev-7684b83`), so that a
// screenshot or a bug report says which. A release is given its number by the build (see `PANIC_AND_IMPROVISE_VERSION`
// in the flake), which wins over this.

use std::process::Command;

fn main() {
    let commit = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    let label = match commit {
        Some(commit) => format!("dev-{commit}"),
        // No git, or no history (as in the sandbox a release is built in).
        None => "dev".to_string(),
    };
    println!("cargo:rustc-env=PANIC_AND_IMPROVISE_DEV_VERSION={label}");
    // Only a new commit changes it.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
}
