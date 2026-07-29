use std::path::Path;
use std::process::Command;

use anyhow::Context;

/// Run `prog` with `args`, inheriting this process's stderr so subprocess
/// output is visible in real time. Bails with the exit status on non-zero exit;
/// the working directory is named too when one was set, since a failure in a
/// temp checkout is undiagnosable without it.
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
        match cwd {
            Some(d) => anyhow::bail!("`{label}` exited with {status} (in {})", d.display()),
            None => anyhow::bail!("`{label}` exited with {status}"),
        }
    }
    Ok(())
}

pub fn git(args: &[&str]) -> anyhow::Result<()> {
    run("git", args)
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
