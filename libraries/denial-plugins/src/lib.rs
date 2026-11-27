//! Denial's plugin manager: chooses plugins, builds the shell with them and
//! asks deniald to run it (Denial's docs/PLUGIN_MANAGER.md). Settings links
//! it and runs requests on its own threads; the `denial-plugins` command
//! adds the command line, background requests and `resume`.
//!
//! Plugin discovery stays Dart: a script of Denial's runtime, run with the
//! Dart of the installation's Flutter tool. Flutter's build tool and Pub
//! stay programs of their own.

pub mod activation;
pub mod available;
pub mod boundary;
pub mod builder;
pub mod discovery;
pub mod generator;
pub mod graph;
pub mod installation;
pub mod manager;
pub mod notifications;
pub mod preflight;
pub mod progress;
pub mod rebuild;
pub mod report;
pub mod resume;
pub mod source;
pub mod store;
pub mod tools;
pub mod util;
pub mod version;
pub mod workspace;

pub use installation::Installation;
pub use manager::{Context, Manager, Options, Outcome, working};
pub use store::Store;
pub use util::{Error, Result};
