use std::path::PathBuf;
use std::process;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Datelike, Local, Month};
use clap::{value_parser, Arg, ArgAction, ArgGroup, Command};
use lazy_static::lazy_static;
use num_traits::FromPrimitive;

mod pledger;

lazy_static! {
    static ref NOW: DateTime<Local> = Local::now();
    static ref NOW_FMT: String = NOW.format("%Y-%m").to_string();
}

fn app() -> Command {
    Command::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .group(
            ArgGroup::new("selector")
                .args(["all", "year", "date", "last"])
                .required(false)
                // NOTE(ww): -d/--date has a default value, so at least one member of selector
                // is always present. Thus, we need `multiple` to keep clap from dying
                // when it sees e.g. --all with an implicit --date.
                .multiple(true),
        )
        .arg(
            Arg::new("all")
                .help("combine all ledgers")
                .short('a')
                .long("all")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("year")
                .help("combine all ledgers from the given year")
                .short('y')
                .long("year")
                .num_args(1),
        )
        .arg(
            Arg::new("date")
                .help("use a ledger by date")
                .short('d')
                .long("date")
                .default_value(<String as AsRef<str>>::as_ref(&NOW_FMT)),
        )
        .arg(
            Arg::new("last")
                .help("use the previous ledger")
                .short('l')
                .long("last")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("edit")
                .help("edit the selected ledger")
                .short('e')
                .long("edit")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("json")
                .help("output in JSON format")
                .short('j')
                .long("json")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("filter")
                .help("produce only ledger entries containing these tags (comma-separated)")
                .short('f')
                .long("filter")
                .num_args(1),
        )
        .arg(
            Arg::new("directory")
                .help("ledger directory")
                .index(1)
                .required(true)
                .value_parser(value_parser!(PathBuf))
                .env("PLEDGER_DIR"),
        )
}

fn run() -> Result<()> {
    let matches = app().get_matches();

    let ledger_dir = matches.get_one::<PathBuf>("directory").unwrap();

    let (all, year, date, last) = (
        matches.get_one::<bool>("all").unwrap(),
        matches.contains_id("year"),
        matches.contains_id("date"),
        matches.get_one::<bool>("last").unwrap(),
    );

    // NOTE(ww): Observe once again that `date` is always true, since it has a default.
    // This is pretty messy; there ought to be a better way to do this.
    let mut ledger = match (all, year, date, last) {
        (true, false, true, false) => {
            pledger::parse_ledger("*", pledger::read_all_ledgers(ledger_dir)?)?
        }
        (false, true, true, false) => {
            let year = matches.get_one::<String>("year").unwrap();
            pledger::parse_ledger(year, pledger::read_ledgers_for_year(ledger_dir, year)?)?
        }
        (false, false, true, true) => {
            let last_month = Month::from_u32(NOW.month())
                .ok_or_else(|| {
                    anyhow!(
                        "unlikely failure converting {} into a chrono::Month",
                        NOW.month()
                    )
                })?
                .pred();

            log::debug!("{:?}", last_month);

            // If we've wrapped back around to December, correct the year as well.
            let year = match last_month {
                Month::December => NOW.year() - 1,
                _ => NOW.year(),
            };

            // NOTE(ww): Without `with_day`, we'd naively jump backyards to an invalid date
            // on some months. For example, July 31st would become June 31st, which isn't a real
            // day. Every month should have a first day, so `with_day(1)` should always succeed.
            let last = NOW
                .with_day(1)
                .and_then(|d| d.with_month(last_month.number_from_month()))
                .and_then(|d| d.with_year(year))
                .ok_or_else(|| anyhow!("datetime calculation for the previous month failed"))?;

            let date = last.format("%Y-%m").to_string();

            // TODO(ww): Dedupe with below.
            if *matches.get_one::<bool>("edit").unwrap() {
                return pledger::edit_ledger(&date, ledger_dir);
            }

            pledger::parse_ledger(&date, pledger::read_ledger(ledger_dir, &date)?)?
        }
        (false, false, true, false) => {
            let date = pledger::parse_date(matches.get_one::<String>("date").unwrap())?;

            if *matches.get_one::<bool>("edit").unwrap() {
                return pledger::edit_ledger(&date, ledger_dir);
            }

            pledger::parse_ledger(&date, pledger::read_ledger(ledger_dir, &date)?)?
        }
        _ => {
            return Err(anyhow!(
                "conflicting uses of --all, --year, --date, or --last"
            ))
        }
    };

    if let Some(filter) = matches.get_one::<String>("filter") {
        let filter: Vec<&str> = filter.split(',').collect();
        ledger.filter(&filter);
    }

    if *matches.get_one::<bool>("json").unwrap() {
        println!("{}", serde_json::to_string(&ledger).unwrap());
    } else {
        pledger::summarize(&ledger);
    }

    Ok(())
}

fn main() {
    env_logger::init();

    process::exit(match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Fatal: {}", e);
            1
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app() {
        app().debug_assert();
    }
}
