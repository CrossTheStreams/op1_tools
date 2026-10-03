use std::io::{self, IsTerminal};

use crate::cli::Args;
use crate::commands::{backup::backup, live::open_in_live, restore::restore, tape::clear_tape};
use crate::device::{mount_status, Scope, OP1_MOUNT};
use crate::ui::prompt;

pub(crate) fn menu() -> io::Result<()> {
    if !io::stdin().is_terminal() {
        return Err(io::Error::other(
            "no command given and stdin is not a terminal. \
             Run `op1_tools --help` for the commands.",
        ));
    }

    println!("op1_tools");
    loop {
        match mount_status() {
            Ok(()) => println!("\nOP-1 connected at '{OP1_MOUNT}'."),
            Err(e) => println!("\n{e}"),
        }

        println!("\n  1) Back up the whole OP-1");
        println!("  2) Restore the whole OP-1 from a backup");
        println!("  3) Back up the tape only");
        println!("  4) Restore the tape only from a backup");
        println!("  5) Delete everything in the tape folder");
        println!("  6) Open a backup's tape in Ableton Live 11");
        println!("  q) Quit");

        let Some(choice) = prompt("\nChoice: ")? else {
            return Ok(());
        };

        // A failed action shouldn't drop the user out of the menu: the usual
        // cause is the OP-1 not being in disk mode yet, which they can fix and
        // then pick again.
        let result = match choice.as_str() {
            "1" => backup(Scope::All, Args::interactive(Scope::All)),
            "2" => restore(Scope::All, Args::interactive(Scope::All)),
            "3" => backup(Scope::Tape, Args::interactive(Scope::Tape)),
            "4" => restore(Scope::Tape, Args::interactive(Scope::Tape)),
            "5" => clear_tape(Args::interactive(Scope::Tape)),
            "6" => open_in_live(Args::interactive(Scope::Tape)),
            "q" | "quit" | "exit" => return Ok(()),
            other => {
                println!("'{other}' isn't one of the choices.");
                continue;
            }
        };
        if let Err(e) = result {
            println!("error: {e}");
        }
    }
}
