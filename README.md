pledger
=======

![license](https://raster.shields.io/badge/license-MIT%20with%20restrictions-green.png)
[![CI](https://github.com/woodruffw/pledger/actions/workflows/ci.yml/badge.svg)](https://github.com/woodruffw/pledger/actions/workflows/ci.yml)

> [!WARNING]
>
> This project has been archived and is not being actively maintained.

A small personal expense ledger.

All `pledger` does is track monthly expenses.

## Installation

`pledger` is a single command-line program. You can install it using `cargo`:

```bash
cargo install pledger
```

Or by building it locally:

```bash
git clone https://github.com/woodruffw/pledger && cd pledger
cargo build
```

## Usage

`pledger` takes only one input: a directory where monthly ledgers are stored:

```bash
pledger expenses/
```

Alternatively, you can use `PLEDGER_DIR` to pass the directory:

```bash
PLEDGER_DIR=expenses/ pledger
```

Ledgers are stored as structured text files with the filename `YYYY-MM.ledger`. Read about the `pledger`
format [below](#ledger-format).

For example, here's a listing for a directory with three months of expenses:

```bash
$ ls expenses/
2018-02.ledger
2020-01.ledger
2020-02.ledger
```

`pledger` ignores files that don't match the `YYYY-MM.ledger` format.

By default, `pledger` reports expenses for the current month.

To run `pledger` on a previous date, use `pledger -d <spec>` or `pledger -l`:

```bash
# do a report on january 2017
pledger -d 2017-01 expenses/

# the month name or single number is also enough for the current year
# do a report on april, then march
pledger -d april expenses/
pledger -d 3 expenses

# -l/--last is a shortcut for last month's ledger
pledger -l expenses/
```

`pledger` can also generate a report for all records with `pledger --all`, or for a specific year
with `pledger --year YEAR`.

By default, pledger outputs a plain text report. You can use the `--json` flag to output JSON
instead, for consumption by other tools:

```bash
pledger --json expenses/ > monthly.json
```

## Ledger format

`pledger`'s ledgers are plain text files, with one entry per line. Debits begin with `D`,
credits with `C`, and the rest of the format is mostly self-explanatory:

```
C 130.00 #bonus
D 8.00 burger and fries #weekday #lunch
D 27.00 saturday drinks #weekend #alcohol
D 20,000.12 new car #essential
```

Everything after the currency amount is the _comment_. The _comment_ can include _tags_, which
begin with `#` and can be alphanumeric + symbolic. `pledger` uses your tags to provide expense
summaries; duplicate tags in a comment are removed.

Empty lines or lines that begin with `#` are ignored.
