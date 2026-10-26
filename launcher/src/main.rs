//! `launcher [--no-update] [game arguments]`: brings the game up to date from its latest GitHub release, then
//! starts it. Anything else on the command line goes to the game (`--host`, for one).

use std::io::{self, Write};
use std::process::{Command, ExitCode};

use launcher::{GITHUB_API, Install, Outcome, REPOSITORY, update};

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let skip_update = args.iter().any(|a| a == "--no-update");
    args.retain(|a| a != "--no-update");
    match run(skip_update, &args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("\n{message}");
            // Run from a double click, the window would close before this could be read.
            eprint!("Press Enter to close.");
            let _ = io::stderr().flush();
            let _ = io::stdin().read_line(&mut String::new());
            ExitCode::FAILURE
        }
    }
}

fn run(skip_update: bool, game_args: &[String]) -> Result<(), String> {
    let installation = Install::usual().map_err(|e| e.to_string())?;
    if !skip_update {
        println!("Looking for a new version of Panic and Improvise...");
        match bring_up_to_date(&installation) {
            Ok(Outcome::Current(tag)) => println!("{tag} is the latest."),
            Ok(Outcome::Updated { to, .. }) => println!("Now on {to}."),
            Ok(Outcome::NothingReleased) => println!("No release has been published yet."),
            // A game that is here can still be played, which is what was wanted.
            Err(e) if installation.installed().is_some() => eprintln!("Could not update ({e}); starting the version already installed."),
            Err(e) => return Err(format!("Could not install the game: {e}")),
        }
    }
    if installation.installed().is_none() {
        return Err("The game is not installed, and there is no release to install it from yet.".into());
    }
    println!("Starting.");
    Command::new(installation.program())
        .args(game_args)
        .current_dir(installation.game_dir())
        .env("BEVY_ASSET_ROOT", installation.game_dir())
        // Where the game keeps what it makes (its recordings), beside what it was installed as.
        .env(launcher::HOME_VARIABLE, &installation.root)
        .spawn()
        .map_err(|e| format!("Could not start {}: {e}", installation.program().display()))?;
    Ok(())
}

fn bring_up_to_date(installation: &Install) -> launcher::Result<Outcome> {
    let mut shown = u64::MAX;
    let mut announced = false;
    let outcome = update(installation, GITHUB_API, REPOSITORY, &mut |done, total| {
        if !announced {
            announced = true;
            println!("A new version is available. Downloading ({:.0} MB)...", total as f64 / 1e6);
        }
        let percent = (done * 100).checked_div(total).unwrap_or(100);
        if percent != shown {
            shown = percent;
            print!("\r  {percent}%");
            let _ = io::stdout().flush();
        }
    })?;
    if announced {
        println!();
    }
    Ok(outcome)
}
