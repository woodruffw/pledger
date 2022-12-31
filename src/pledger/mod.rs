use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Result};
use chrono::Utc;
use lazy_static::lazy_static;
use phf::phf_map;
use regex::Regex;
use serde::ser::SerializeTuple;
use serde::{Serialize, Serializer};

use crate::pledger::EntryKind::*;
use crate::pledger::EntryParseState::*;

type LedgerLines = Box<dyn Iterator<Item = io::Result<String>>>;

pub static MONTH_MAP: phf::Map<&'static str, u8> = phf_map! {
    "jan" => 1,
    "january" => 1,
    "feb" => 2,
    "february" => 2,
    "mar" => 3,
    "march" => 3,
    "apr" => 4,
    "april" => 4,
    "may" => 5,
    "jun" => 6,
    "june" => 6,
    "jul" => 7,
    "july" => 7,
    "aug" => 8,
    "august" => 8,
    "sep" => 9,
    "september" => 9,
    "oct" => 10,
    "october" => 10,
    "nov" => 11,
    "november" => 11,
    "dec" => 12,
    "december" => 12,
};

lazy_static! {
    static ref DATE_PATTERN: Regex = Regex::new(r"^\d{4}-(0[1-9]|1[0-2])$").unwrap();
}

#[derive(Copy, Clone, Debug)]
enum EntryParseState {
    Whitespace,
    EntryKind,
    Amount,
    Comment,
    Tag,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
enum EntryKind {
    Debit,
    Credit,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct Entry {
    kind: EntryKind,
    #[serde(serialize_with = "amount_serialize")]
    amount: u64,
    comment: String,
    tags: Vec<String>,
}

fn amount_serialize<S>(amount: &u64, s: S) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let subunits: u64 = amount % 100;
    let units: u64 = amount / 100;
    let mut tup = s.serialize_tuple(2)?;
    tup.serialize_element(&units)?;
    tup.serialize_element(&subunits)?;
    tup.end()
}

fn amount_format(amount: &u64) -> String {
    let subunits: u64 = amount % 100;
    let units: u64 = amount / 100;

    format!("{:02}.{:02}", units, subunits)
}

#[derive(Debug, Serialize)]
pub struct Ledger {
    date: String,
    entries: Vec<Entry>,
}

impl Ledger {
    pub fn filter(&mut self, tags: &[&str]) {
        self.entries
            .retain(|e| e.tags.iter().any(|t| tags.contains(&t.as_ref())));
    }
}

pub fn parse_date(date: &str) -> Result<String> {
    // First: is our date already totally formed? If it is, just return it.
    if DATE_PATTERN.is_match(date) {
        return Ok(date.to_string());
    }

    // Next: is our date in the MONTH_MAP? If it is, build it.
    if MONTH_MAP.contains_key(date) {
        return Ok(format!(
            "{}-{:02}",
            Utc::now().format("%Y"),
            MONTH_MAP.get(date).unwrap()
        ));
    }

    // Finally: is our date a number corresponding to a month? If it is, use it.
    match date.parse::<u8>() {
        Ok(month) if (1..=12).contains(&month) => {
            Ok(format!("{}-{:02}", Utc::now().format("%Y"), month))
        }
        Ok(month) => Err(anyhow!("month out of range: {}", month)),
        Err(_) => Err(anyhow!("failed to parse supplied date: {}", date)),
    }
}

pub fn read_ledger(directory: &Path, date: &str) -> Result<LedgerLines> {
    if !directory.is_dir() {
        return Err(anyhow!("invalid ledger directory: {}", directory.display()));
    }

    let ledger_file = directory.join(format!("{}.ledger", date));
    if !ledger_file.is_file() {
        return Err(anyhow!(
            "missing requested ledger file: {}",
            ledger_file.display()
        ));
    }

    match fs::File::open(ledger_file) {
        Ok(file) => Ok(Box::new(io::BufReader::new(file).lines())),
        Err(e) => Err(anyhow!("ledger file read failed: {}", e)),
    }
}

pub fn read_all_ledgers(directory: &Path) -> Result<LedgerLines> {
    let mut ledger_iters = vec![];
    for entry in fs::read_dir(directory)? {
        let entry = entry?.path();

        let date: String = match entry.extension().and_then(OsStr::to_str) {
            Some("ledger") => entry
                .with_extension("")
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            Some(_) => continue,
            None => continue,
        };

        if !DATE_PATTERN.is_match(&date) {
            log::debug!("skipping non-date file: {:?}", entry);
            continue;
        }

        ledger_iters.push(read_ledger(directory, &date)?);
    }

    Ok(ledger_iters
        .into_iter()
        .fold(Box::new(std::iter::empty()) as LedgerLines, |acc, e| {
            Box::new(acc.chain(e))
        }))
}

pub fn read_ledgers_for_year(directory: &Path, year: &str) -> Result<LedgerLines> {
    let mut ledger_iters = vec![];
    for entry in fs::read_dir(directory)? {
        let entry = entry?.path();
        let date: String = match entry.extension().and_then(OsStr::to_str) {
            Some("ledger") => entry
                .with_extension("")
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            Some(_) => continue,
            None => continue,
        };

        if !DATE_PATTERN.is_match(&date) {
            log::debug!("skipping non-date file: {:?}", entry);
            continue;
        }

        if !date.starts_with(year) {
            continue;
        }

        ledger_iters.push(read_ledger(directory, &date)?);
    }

    Ok(ledger_iters
        .into_iter()
        .fold(Box::new(std::iter::empty()) as LedgerLines, |acc, e| {
            Box::new(acc.chain(e))
        }))
}

pub fn edit_ledger(date: &str, ledger_dir: &Path) -> Result<()> {
    let editor = match env::var("EDITOR") {
        Ok(e) => e,
        Err(e) => return Err(anyhow!("EDITOR lookup failed: {}", e)),
    };

    let ledger_file = Path::new(ledger_dir).join(date);
    if let Ok(status) = Command::new(editor.clone()).arg(ledger_file).status() {
        if status.success() {
            Ok(())
        } else {
            Err(anyhow!("EDITOR exited with: {}", status))
        }
    } else {
        Err(anyhow!("failed to execute EDITOR: {}", editor))
    }
}

// TODO(ww): Maybe use PEGs or combinators here. Or maybe not. It's not a very complicated parser.
pub fn parse_ledger(date: &str, ledger_lines: LedgerLines) -> Result<Ledger> {
    let mut entries = Vec::new();
    for (idx, line) in ledger_lines.enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(e) => return Err(anyhow!("ledger read failed: {}", e)),
        };

        match parse_entry(&line) {
            Ok(entry) => {
                log::debug!("entry: {:?}", entry);
                entries.push(entry);
            }
            Err(o) => match o {
                None => continue, // No error, just an empty line or comment.
                Some(e) => {
                    return Err(anyhow!("parse error on line {}: {}", idx + 1, e));
                }
            },
        }
    }

    #[allow(clippy::redundant_field_names)]
    Ok(Ledger {
        date: String::from(date),
        entries: entries,
    })
}

fn parse_entry(line: &str) -> std::result::Result<Entry, Option<String>> {
    lazy_static! {
        static ref LOOKS_LIKE_COMMENT: Regex = Regex::new(r"^\s*#.*$").unwrap();
    }

    if line.is_empty() || LOOKS_LIKE_COMMENT.is_match(line) {
        log::debug!("comment or blank: {}", line);
        return Err(None);
    }

    // Parser transitions.
    let (mut prev_state, mut cur_state) = (EntryKind, EntryKind);

    // Parser state.
    // NOTE(ww): kind is (pointlessly) initialized to Debit because Rust isn't clever enough to see
    // that we always initialize it below.
    let mut kind = Debit;
    let mut amount = 0_u64;
    let mut in_decimal_place = false;
    let mut decimal_place = 0;
    let mut comment = String::new();
    let mut tags: Vec<String> = Vec::new();

    for (idx, chr) in line.char_indices() {
        log::debug!("parser transition: {:?} => {:?}", prev_state, cur_state);
        match (prev_state, cur_state) {
            (EntryKind, EntryKind) => {
                kind = match chr {
                    'C' => Credit,
                    'D' => Debit,
                    _ => {
                        return Err(Some(format!(
                            "offset {}: unexpected entry kind {}",
                            idx, chr
                        )))
                    }
                };
                cur_state = Whitespace;
            }
            (EntryKind, Whitespace) => {
                if chr.is_ascii_whitespace() {
                    prev_state = Whitespace;
                    cur_state = Amount;
                } else {
                    return Err(Some(format!(
                        "offset {}: expected whitespace, got {}",
                        idx, chr
                    )));
                }
            }
            (Whitespace, Amount) => {
                if chr.is_ascii_digit() {
                    amount *= 10;
                    amount += chr as u64 - '0' as u64;
                    prev_state = Amount;
                } else {
                    return Err(Some(format!("offset {}: expected digit, got {}", idx, chr)));
                }
            }
            (Amount, Amount) => {
                if chr.is_ascii_digit() {
                    if in_decimal_place {
                        decimal_place += 1;
                    }
                    if decimal_place > 2 {
                        return Err(Some(format!(
                            "offset {}: more than two decimal places in value",
                            idx
                        )));
                    }
                    amount *= 10;
                    amount += chr as u64 - '0' as u64;
                } else if chr == '.' {
                    if in_decimal_place {
                        return Err(Some(format!(
                            "offset {}: more than one decimal supplied in value",
                            idx
                        )));
                    } else {
                        in_decimal_place = true;
                    }
                } else if chr == ',' {
                    // NOTE(ww): We could count places here to make sure that commas
                    // are inserted in reasonable locations, but that would complicate the parser.
                    continue;
                } else if chr.is_ascii_whitespace() {
                    if in_decimal_place && decimal_place < 2 {
                        return Err(Some(format!(
                            "offset {}: one or more decimals missing from decimal place",
                            idx
                        )));
                    }
                    // NOTE(ww): More state transition cheating -- we've just consumed
                    // the whitespace, so there's no point in wasting another state on it.
                    prev_state = Comment;
                    cur_state = Comment;
                } else {
                    return Err(Some(format!(
                        "offset {}: expected digit or whitespace, got {}",
                        idx, chr
                    )));
                }
            }
            (Comment, Comment) => {
                if chr == '#' {
                    let tag = String::from("#");
                    tags.push(tag);
                    cur_state = Tag;
                }
                comment.push(chr);
            }
            (Comment, Tag) => {
                if chr.is_ascii_whitespace() {
                    return Err(Some(format!("offset {}: premature tag ending", idx)));
                } else if chr.is_ascii_graphic() {
                    // Add the current character to both the comment and
                    // the most recent tag.
                    comment.push(chr);
                    tags.last_mut().unwrap().push(chr);

                    prev_state = Tag;
                } else {
                    return Err(Some(format!(
                        "offset {}: invalid tag character: {}",
                        idx, chr
                    )));
                }
            }
            (Tag, Tag) => {
                if chr.is_ascii_whitespace() {
                    comment.push(chr);

                    // NOTE(ww): Again, a little cheating: we pretend we've already begun
                    // the comment to avoid a completely duplicated (Tag, Comment)
                    // transition.
                    prev_state = Comment;
                    cur_state = Comment;
                } else if chr.is_ascii_graphic() {
                    comment.push(chr);
                    tags.last_mut().unwrap().push(chr);
                } else {
                    return Err(Some(format!(
                        "offset {}: invalid tag character: {}",
                        idx, chr
                    )));
                }
            }
            (_, _) => {
                return Err(Some(format!(
                    "unexpected parser state transition: {:?} => {:?}! probable bug.",
                    prev_state, cur_state
                )))
            }
        }
    }

    // Tag order is not preserved, and duplicate tags are not preserved.
    tags.sort_unstable();
    tags.dedup();

    match (prev_state, cur_state) {
        (Comment, Comment) | (Tag, Tag) => Ok(Entry {
            kind,
            amount,
            comment,
            tags,
        }),
        (_, _) => Err(Some("unexpected EOL; missing comment?".into())),
    }
}

pub fn summarize(ledger: &Ledger) {
    println!("Ledger for {}\n", ledger.date);
    println!("Summary:");

    let num_entries = ledger.entries.len();
    let total_credits = ledger
        .entries
        .iter()
        .filter(|e| e.kind == Credit)
        .fold(0, |acc, e| acc + e.amount);
    let total_debits = ledger
        .entries
        .iter()
        .filter(|e| e.kind == Debit)
        .fold(0, |acc, e| acc + e.amount);

    let (net, kind) = if total_credits >= total_debits {
        (total_credits - total_debits, "credit")
    } else {
        (total_debits - total_credits, "debit")
    };

    println!(
        "\t{} entries, totaling {} in credits and {} in debits for a net of {} in {}\n",
        num_entries,
        amount_format(&total_credits),
        amount_format(&total_debits),
        amount_format(&net),
        kind
    );

    let mut tags_by_credit = HashMap::new();
    let mut tags_by_debit = HashMap::new();

    for entry in ledger.entries.iter() {
        let map = match entry.kind {
            Credit => &mut tags_by_credit,
            Debit => &mut tags_by_debit,
        };

        for tag in entry.tags.iter() {
            let tag_value = map.entry(tag).or_insert(0);
            *tag_value += entry.amount;
        }
    }

    let mut sorted_credits: Vec<_> = tags_by_credit.iter().collect();
    sorted_credits.sort_by(|a, b| b.1.cmp(a.1));

    let mut sorted_debits: Vec<_> = tags_by_debit.iter().collect();
    sorted_debits.sort_by(|a, b| b.1.cmp(a.1));

    println!("Top credit tags:");
    for credit in sorted_credits.iter() {
        println!("{:<16} {:>10}", credit.0, amount_format(credit.1));
    }

    println!("\nTop debit tags:");
    for credit in sorted_debits.iter() {
        println!("{:<16} {:>10}", credit.0, amount_format(credit.1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_date() {
        let current_year = Utc::now().format("%Y").to_string();

        assert_eq!(
            parse_date(format!("{}-01", current_year).as_str()).unwrap(),
            format!("{}-01", current_year)
        );
        assert_eq!(
            parse_date("january").unwrap(),
            format!("{}-01", current_year)
        );
        assert_eq!(parse_date("jan").unwrap(), format!("{}-01", current_year));
        assert_eq!(parse_date("1").unwrap(), format!("{}-01", current_year));
        assert_eq!(parse_date("01").unwrap(), format!("{}-01", current_year));

        assert_eq!(
            parse_date("13").unwrap_err().to_string(),
            "month out of range: 13"
        );
        assert_eq!(
            parse_date("not_a_real_month").unwrap_err().to_string(),
            "failed to parse supplied date: not_a_real_month"
        );
    }

    #[test]
    fn test_parse_entry() {
        // Whitespace and comments.
        assert_eq!(parse_entry(""), Err(None));
        assert_eq!(parse_entry("# this is a comment"), Err(None));
        assert_eq!(parse_entry("   # this is a comment"), Err(None));

        // Misc. syntax errors.
        assert_eq!(
            parse_entry("D1.00"),
            Err(Some("offset 1: expected whitespace, got 1".to_string()))
        );
        assert_eq!(
            parse_entry("D 1.00foo"),
            Err(Some(
                "offset 6: expected digit or whitespace, got f".to_string()
            ))
        );

        // Entry kinds.
        assert_eq!(
            parse_entry("X 1.00 test"),
            Err(Some("offset 0: unexpected entry kind X".to_string()))
        );

        let entry = parse_entry("C 1.00 test").unwrap();
        assert_eq!(entry.kind, EntryKind::Credit);

        let entry = parse_entry("D 1.00 test").unwrap();
        assert_eq!(entry.kind, EntryKind::Debit);

        // Amounts.
        assert_eq!(
            parse_entry("D abc"),
            Err(Some("offset 2: expected digit, got a".to_string()))
        );
        assert_eq!(
            parse_entry("D 1.000"),
            Err(Some(
                "offset 6: more than two decimal places in value".to_string()
            ))
        );
        assert_eq!(
            parse_entry("D 1.0.0"),
            Err(Some(
                "offset 5: more than one decimal supplied in value".to_string()
            ))
        );

        let entry = parse_entry("C 1.00 test").unwrap();
        assert_eq!(entry.amount, 100);

        let entry = parse_entry("D 100.00 test").unwrap();
        assert_eq!(entry.amount, 10000);

        let entry = parse_entry("C 100 test").unwrap();
        assert_eq!(entry.amount, 100);

        // Comments and tags.
        assert_eq!(
            parse_entry("D 1"),
            Err(Some("unexpected EOL; missing comment?".to_string()))
        );
        assert_eq!(
            parse_entry("D 1 # bar"),
            Err(Some("offset 5: premature tag ending".to_string()))
        );
        assert_eq!(
            parse_entry("D 1 foo # bar"),
            Err(Some("offset 9: premature tag ending".to_string()))
        );
        assert_eq!(
            parse_entry("D 1 foo #\x01"),
            Err(Some("offset 9: invalid tag character: \x01".to_string()))
        );
        assert_eq!(
            parse_entry("D 1 #foo #\x01"),
            Err(Some("offset 10: invalid tag character: \x01".to_string()))
        );

        let entry = parse_entry("C 1.00 foo bar baz").unwrap();
        assert_eq!(entry.comment, "foo bar baz".to_string());
        assert_eq!(entry.tags, Vec::<String>::new());

        let entry = parse_entry("C 1.00 foo #bar baz").unwrap();
        assert_eq!(entry.comment, "foo #bar baz".to_string());
        assert_eq!(entry.tags, vec!["#bar"]);

        let entry = parse_entry("C 1.00 foo #bar #baz").unwrap();
        assert_eq!(entry.comment, "foo #bar #baz".to_string());
        assert_eq!(entry.tags, vec!["#bar", "#baz"]);

        let entry = parse_entry("C 1.00 #foo").unwrap();
        assert_eq!(entry.comment, "#foo".to_string());
        assert_eq!(entry.tags, vec!["#foo"]);
    }

    #[test]
    fn test_parse_ledger() {
        // NOTE(ww): as_bytes() makes us use `BufRead.lines` instead of `str.lines`.
        let ledger = parse_ledger(
            "01-01-1970",
            Box::new("C 1.00 #foo\nD 1.00 #bar".as_bytes().lines()),
        )
        .unwrap();

        assert_eq!(ledger.entries.len(), 2);
        assert_eq!(ledger.date, "01-01-1970");
    }

    #[test]
    fn test_filter_ledger() {
        let mut ledger = parse_ledger(
            "01-01-1970",
            Box::new("C 1.00 #foo\nD 1.00 #bar".as_bytes().lines()),
        )
        .unwrap();

        ledger.filter(&["#foo"]);

        assert_eq!(ledger.entries.len(), 1);
        assert_eq!(ledger.entries[0].kind, EntryKind::Credit);
    }
}
