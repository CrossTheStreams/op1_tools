mod cli;
mod commands;
mod device;
mod menu;
mod store;
mod ui;
mod util;

use std::io;
use std::process::ExitCode;

use cli::{print_help, Args};
use commands::{backup::backup, live::open_in_live, restore::restore, tape::clear_tape};
use device::Scope;
use menu::menu;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();

    match command.as_str() {
        "backup" => backup(Scope::All, Args::parse(Scope::All, args)?),
        "restore" => restore(Scope::All, Args::parse(Scope::All, args)?),
        "backup-tape" => backup(Scope::Tape, Args::parse(Scope::Tape, args)?),
        "restore-tape" => restore(Scope::Tape, Args::parse(Scope::Tape, args)?),
        "clear-tape" => clear_tape(Args::parse(Scope::Tape, args)?),
        "live" => open_in_live(Args::parse(Scope::Tape, args)?),
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        "" => menu(),
        other => Err(io::Error::other(format!(
            "unknown command '{other}'. Run `op1_tools --help` for usage."
        ))),
    }
}
