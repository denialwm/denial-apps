//! `denial-plugins`: the plugin manager's command line. Settings links the
//! same library; this adds what a terminal and deniald ask for.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use denial_plugins::manager::{OPTIONS, USAGE};
use denial_plugins::store::process_start;
use denial_plugins::util::{self, absolute, display};
use denial_plugins::{Context, Installation, Manager, Options, Result, Store, object};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("denial-plugins: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let options = Options::parse(&arguments)?;
    if options.help || options.rest.is_empty() {
        println!("{USAGE}\n{OPTIONS}");
        return Ok(());
    }
    let store = Store::new(match &options.state {
        Some(state) => absolute(state),
        None => Store::default_root(),
    });
    let manager = Manager::new(store, Installation::beside_executable()?);
    let rest: Vec<&str> = options.rest.iter().map(String::as_str).collect();
    let outcome = match rest.as_slice() {
        // A request that goes on after this command ends, in a process of
        // its own; `status` reports it.
        ["submit", operation @ ..] => {
            let mut argv: Vec<String> = operation
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect();
            if let Some(reference) = &options.reference {
                argv.extend(["--ref".into(), reference.clone()]);
            }
            if let Some(path) = &options.path {
                argv.extend(["--path".into(), path.clone()]);
            }
            for (flag, on) in [
                ("local", options.local),
                ("builtin", options.builtin),
                ("update", options.update),
                ("now", options.now),
            ] {
                if on {
                    argv.push(format!("--{flag}"));
                }
            }
            let id = manager.queue(&argv)?;
            if let Err(error) = spawn_worker(&manager, &id) {
                manager.store.update_job(
                    &id,
                    object! {"phase" => "failed", "error" => error.message.clone()},
                )?;
                return Err(error);
            }
            println!("{}", util::pretty(&object! {"job" => id}.into()));
            return Ok(());
        }
        ["worker", id] => {
            manager.run_request(id);
            return Ok(());
        }
        ["resume"] => return manager.resume(),
        _ => manager.execute(&options, &Context::terminal())?,
    };
    // People read text; programs read JSON.
    let json = options.json || options.brief;
    match outcome.text {
        Some(text) if !json => println!("{text}"),
        _ => println!("{}", util::pretty(&outcome.result)),
    }
    Ok(())
}

/// Starts `denial-plugins worker ID`, detached from this command.
fn spawn_worker(manager: &Manager, id: &str) -> Result<()> {
    let executable: PathBuf = std::env::current_exe()?;
    let child = Command::new(executable)
        .args(["--state", &display(&manager.store.root), "worker", id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    manager.store.write(
        &format!("jobs/{id}.spawn"),
        object! {"pid" => child.id(), "processStart" => process_start(child.id())},
    )
}
