use std::path::Path;
use std::process::Command;

use anyhow::Context;

/// Run `prog` with `args`, inheriting this process's stderr so subprocess
/// output is visible in real time. Bails with the exit status on non-zero exit.
pub fn run(prog: &str, args: &[&str]) -> anyhow::Result<()> {
    run_inner(prog, args, None)
}

pub fn run_in(cwd: &Path, prog: &str, args: &[&str]) -> anyhow::Result<()> {
    run_inner(prog, args, Some(cwd))
}

fn run_inner(prog: &str, args: &[&str], cwd: Option<&Path>) -> anyhow::Result<()> {
    let mut cmd = Command::new(prog);
    cmd.args(args);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    let label = format!("{prog} {}", args.join(" "));
    tracing::info!(target: "mise::process", "{label}");
    let status = cmd.status().with_context(|| format!("spawn `{label}`"))?;
    if !status.success() {
        anyhow::bail!("`{label}` exited with {status}");
    }
    Ok(())
}

pub fn git(args: &[&str]) -> anyhow::Result<()> {
    run("git", args)
}
pub fn pixi_run(args: &[&str]) -> anyhow::Result<()> {
    let mut all = vec!["run"];
    all.extend_from_slice(args);
    run("pixi", &all)
}
pub fn rattler_build(args: &[&str]) -> anyhow::Result<()> {
    pixi_run(&[&["rattler-build"], args].concat())
}
pub fn vinca(args: &[&str]) -> anyhow::Result<()> {
    pixi_run(&[&["vinca"], args].concat())
}
pub fn gh_cli(args: &[&str]) -> anyhow::Result<()> {
    run("gh", args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_succeeds() {
        run("true", &[]).unwrap();
    }

    #[test]
    fn run_propagates_failure() {
        let err = run("false", &[]).unwrap_err();
        assert!(format!("{err}").contains("exited with"));
    }

    #[test]
    fn run_in_uses_cwd() {
        run_in(Path::new("/"), "ls", &["-d", "/"]).unwrap();
    }
}
