use std::io::{self, Write};
use std::path::PathBuf;

/// Read a trimmed line, or `None` at end of input.
pub(crate) fn prompt(text: &str) -> io::Result<Option<String>> {
    print!("{text}");
    io::stdout().flush()?;
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer)? == 0 {
        println!();
        return Ok(None);
    }
    Ok(Some(answer.trim().to_string()))
}

/// Ask for confirmation, accepting only `expected` (case-insensitively).
pub(crate) fn confirm(text: &str, expected: &str) -> io::Result<bool> {
    let Some(answer) = prompt(&format!("{text} "))? else {
        return Ok(false);
    };
    let answer = answer.to_ascii_lowercase();
    Ok(answer == expected || (expected == "y" && answer == "yes"))
}

/// Print a numbered list, oldest first with the last marked, and return the pick.
pub(crate) fn choose_row(heading: &str, rows: &[(String, PathBuf)]) -> io::Result<PathBuf> {
    println!("\n{heading}");
    for (i, (label, _)) in rows.iter().enumerate() {
        let newest = if i + 1 == rows.len() { "  (newest)" } else { "" };
        println!("  {}) {label}{newest}", i + 1);
    }

    let answer = prompt(&format!("Which one? [1-{}] ", rows.len()))?;
    let choice: usize = answer
        .unwrap_or_default()
        .parse()
        .ok()
        .filter(|&c| c >= 1 && c <= rows.len())
        .ok_or_else(|| io::Error::other("not one of the listed backups"))?;

    Ok(rows[choice - 1].1.clone())
}

pub(crate) fn files(count: usize) -> String {
    format!("{count} file{}", if count == 1 { "" } else { "s" })
}
