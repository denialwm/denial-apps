//! Denial's Polkit authentication agent and its dialogs: one program.
//!
//! Without arguments it is the agent (`agent`): it registers with polkitd
//! for the login session and, for each request, starts itself again with
//! `--prompt` to draw the password or PIN dialog. denial-portal starts it
//! with `--access` to ask whether an app may have something. Each dialog
//! talks to whoever started it over a private socket the environment names.

#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]

mod agent;
mod apps;
mod dialog;
mod glibc;
mod identity;
mod words;

const USAGE: &str = "denial-polkit-agent [--prompt | --access]
  (none)     Register as the session's Polkit agent; open the dialog on demand
  --prompt   The authentication dialog, started by the agent
  --access   denial-portal's Access dialog, started by the portal";

fn main() {
    let mut arguments = std::env::args().skip(1);
    let mode = arguments.next();
    if let Some(extra) = arguments.next() {
        eprintln!("denial-polkit-agent: unexpected argument {extra}\n{USAGE}");
        std::process::exit(2);
    }
    match mode.as_deref() {
        None => {
            if let Err(error) = agent::run() {
                eprintln!("denial-polkit-agent: {error}");
                std::process::exit(1);
            }
        }
        Some("--prompt") => dialog::run(dialog::Kind::Prompt),
        Some("--access") => dialog::run(dialog::Kind::Access),
        Some("--help" | "-h") => println!("{USAGE}"),
        Some(other) => {
            eprintln!("denial-polkit-agent: unknown option {other}\n{USAGE}");
            std::process::exit(2);
        }
    }
}
