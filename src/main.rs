use std::path::Path;
use std::process;

use anyhow::{anyhow, Result};
use chrono::{Datelike, Local, Month};
use clap::{App, Arg, ArgGroup};
use num_traits::FromPrimitive;

mod pledger;

fn run() -> Result<()> {
    let now = Local::now();
    let matches = App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .group(
            ArgGroup::new("selector")
                .args(&["all", "date", "last"])
                .required(false)
                // NOTE(ww): -d/--date has a default value, so at least one member of selector
                // is always present. Thus, we need `multiple` to keep clap from dying
                // when it sees e.g. --all with an implicit --date.
                .multiple(true),
        )
        .arg(
            Arg::new("all")
                .about("combine all ledgers")
                .short('a')
                .long("all")
                .multiple(false),
        )
        .arg(
            Arg::new("date")
                .about("use a ledger by date")
                .short('d')
                .long("date")
                .multiple(false)
                .default_value(&now.format("%Y-%m").to_string()),
        )
        .arg(
            Arg::new("last")
                .about("use the previous ledger")
                .short('l')
                .long("last")
                .multiple(false),
        )
        .arg(
            Arg::new("edit")
                .about("edit the selected ledger")
                .short('e')
                .long("edit")
                .multiple(false),
        )
        .arg(
            Arg::new("json")
                .about("output in JSON format")
                .short('j')
                .long("json")
                .multiple(false),
        )
        .arg(
            Arg::new("filter")
                .about("produce only ledger entries containing these tags (comma-separated)")
                .short('f')
                .long("filter")
                .multiple(false)
                .takes_value(true),
        )
        .arg(
            Arg::new("directory")
                .about("ledger directory")
                .index(1)
                .required(true)
                .multiple(false)
                .env("PLEDGER_DIR"),
        )
        .get_matches();

    let ledger_dir = Path::new(matches.value_of("directory").unwrap());

    let (all, date, last) = (
        matches.is_present("all"),
        matches.is_present("date"),
        matches.is_present("last"),
    );

    // NOTE(ww): Observe once again that `date` is always true, since it has a default.
    // This is pretty messy; there ought to be a better way to do this.
    let mut ledger = match (all, date, last) {
        (true, true, false) => pledger::parse_ledger("*", pledger::read_ledgers(ledger_dir)?)?,
        (false, true, true) => {
            let last_month = Month::from_u32(now.month())
                .ok_or_else(|| {
                    anyhow!(
                        "unlikely failure converting {} into a chrono::Month",
                        now.month()
                    )
                })?
                .pred();

            log::debug!("{:?}", last_month);

            // NOTE(ww): Without `with_day`, we'd naively jump backyards to an invalid date
            // on some months. For example, July 31st would become June 31st, which isn't a real
            // day. Every month should have a first day, so `with_day(1)` should always succeed.
            let last = now
                .with_day(1)
                .and_then(|d| d.with_month(last_month.number_from_month()))
                .ok_or_else(|| anyhow!("datetime calculation for the previous month failed"))?;

            let date = last.format("%Y-%m").to_string();

            // TODO(ww): Dedupe with below.
            if matches.is_present("edit") {
                return pledger::edit_ledger(&date, ledger_dir);
            }

            pledger::parse_ledger(&date, pledger::read_ledger(ledger_dir, &date)?)?
        }
        (false, true, false) => {
            let date = pledger::parse_date(matches.value_of("date").unwrap())?;

            if matches.is_present("edit") {
                return pledger::edit_ledger(&date, ledger_dir);
            }

            pledger::parse_ledger(&date, pledger::read_ledger(ledger_dir, &date)?)?
        }
        _ => return Err(anyhow!("conflicting uses of --all, --date, or --last")),
    };

    if let Some(filter) = matches.value_of("filter") {
        let filter: Vec<&str> = filter.split(',').collect();
        ledger.filter(&filter);
    }

    if matches.is_present("json") {
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
