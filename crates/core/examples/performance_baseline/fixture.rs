//! Fixed synthetic workload. Integer cents provide an oracle independent of Decimal arithmetic.
use std::fmt::Write;
use workbench_core::domain::ReviewState;

pub const VERSION: &str = "synthetic-transactions-v1";
pub fn frozen_sha256(rows: usize) -> &'static str {
    match rows {
        1000 => "7b1b5fe662bc6dbe7163c99d6688037a7eb13659db8ee61f5852d20230ea8aa3",
        100_000 => "c587e3ad59c11445f780306b9aef370e4d97310cf66728b1aabe5f9819c5aef7",
        _ => "unsupported",
    }
}

pub struct Row {
    pub account: String,
    pub date: String,
    pub description: String,
    pub cents: i64,
    pub currency: &'static str,
    pub review: ReviewState,
    pub transfer: bool,
}

pub fn row(index: usize) -> Row {
    // Keep the same 100-month history at both the 1,000-row smoke and 100,000-row baseline.
    let month = index % 100;
    let merchant = index / 100;
    let source = if merchant == 997 { 996 } else { merchant };
    let transfer = merchant >= 998;
    let positive = if transfer {
        merchant == 999
    } else {
        source % 10 == 5 || source % 10 == 8
    };
    let cents = if transfer {
        7700
    } else {
        1500 + (source % 97) as i64
    };
    let kind = if transfer {
        "TRANSFER"
    } else if source % 10 == 4 {
        "ATM"
    } else if source % 10 == 5 {
        "REFUND"
    } else {
        "MERCHANT"
    };
    Row {
        account: format!("{:04}", source % 8),
        date: format!(
            "{:04}-{:02}-{:02}",
            2016 + month / 12,
            month % 12 + 1,
            source % 28 + 1
        ),
        description: format!("SYNTHETIC {kind} {source:04}"),
        cents: if positive { cents } else { -cents },
        currency: if !transfer && source % 3 == 0 {
            "USD"
        } else {
            "AUD"
        },
        review: match merchant % 20 {
            0 => ReviewState::Pending,
            1 => ReviewState::Rejected,
            2 => ReviewState::Deferred,
            _ => ReviewState::Accepted,
        },
        transfer,
    }
}

pub fn money(cents: i64) -> String {
    format!(
        "{}{}.{:02}",
        if cents < 0 { "-" } else { "" },
        cents.abs() / 100,
        cents.abs() % 100
    )
}

pub fn csv(rows: usize) -> Vec<u8> {
    let mut text = String::from("account,date,description,amount,currency\n");
    for i in 0..rows {
        let r = row(i);
        writeln!(
            text,
            "{},{},{},{},{}",
            r.account,
            r.date,
            r.description,
            money(r.cents),
            r.currency
        )
        .unwrap();
    }
    text.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_fixture_has_canonical_dates_reviews_and_duplicate_pair() {
        let mut states = [0; 4];
        for i in 0..100_000 {
            let r = row(i);
            workbench_core::analytics::date(&r.date).unwrap();
            states[match r.review {
                ReviewState::Accepted => 0,
                ReviewState::Pending => 1,
                ReviewState::Rejected => 2,
                ReviewState::Deferred => 3,
            }] += 1;
        }
        assert_eq!(states, [85_000, 5_000, 5_000, 5_000]);
        assert_eq!(row(99_600).description, row(99_700).description);
        assert_eq!(row(99_800).cents, -row(99_900).cents);
        assert_ne!(row(99_800).account, row(99_900).account);
        assert_eq!(row(99).date, "2024-04-01");
        assert!(csv(100_000).len() < 16 * 1024 * 1024);
        for size in [1000, 100_000] {
            assert_eq!(super::super::digest(&csv(size)), frozen_sha256(size));
        }
    }
}
