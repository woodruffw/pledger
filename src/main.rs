use chrono::Local;
use clap::{App, Arg};

use std::process;

mod pledger;

fn run() -> Result<(), String> {
    let date = Local::now().format("%Y-%m").to_string();
    let matches = App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .arg(
            Arg::with_name("date")
                .help("use a previous ledger")
                .short("d")
                .long("date")
                .multiple(false)
                .default_value(&date),
        )
        .arg(
            Arg::with_name("edit")
                .help("edit the selected ledger")
                .short("e")
                .long("edit")
                .multiple(false),
        )
        .arg(
            Arg::with_name("json")
                .help("output in JSON format")
                .short("j")
                .long("json")
                .multiple(false),
        )
        .arg(
            Arg::with_name("directory")
                .help("ledger directory")
                .index(1)
                .required(true)
                .multiple(false)
                .env("PLEDGER_DIR"),
        )
        .get_matches();

    let date = pledger::parse_date(matches.value_of("date").unwrap())?;
    let ledger_dir = matches.value_of("directory").unwrap();

    if matches.is_present("edit") {
        return pledger::edit_ledger(&date, &ledger_dir);
    }

    let ledger = pledger::parse_ledger(&date, pledger::read_ledger(&ledger_dir, &date)?)?;

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
