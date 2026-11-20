//! How dates are written in the day headings (left pane, tray flyout) and the
//! long date in the Day view and tray tooltip.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum DateFormat {
    DdMmYyyy,
    DdMmYy,
    DdMmYyyyDash,
    DdMmmYyyy,
    MmDdYyyy,
    MmDdYy,
    MmDdYyyyDash,
    MmmDdYyyy,
    MmmmDYyyy,
    YyyyMmDd,
    YyyyMmDdDash,
    DdMm,
    MmDd,
    #[default]
    DdMmm,
    MmmDd,
}

impl DateFormat {
    pub const ALL: [DateFormat; 15] = [
        DateFormat::DdMmYyyy,
        DateFormat::DdMmYy,
        DateFormat::DdMmYyyyDash,
        DateFormat::DdMmmYyyy,
        DateFormat::MmDdYyyy,
        DateFormat::MmDdYy,
        DateFormat::MmDdYyyyDash,
        DateFormat::MmmDdYyyy,
        DateFormat::MmmmDYyyy,
        DateFormat::YyyyMmDd,
        DateFormat::YyyyMmDdDash,
        DateFormat::DdMm,
        DateFormat::MmDd,
        DateFormat::DdMmm,
        DateFormat::MmmDd,
    ];

    /// The pattern as shown to the user, e.g. "DD MMM".
    pub fn pattern(self) -> &'static str {
        use DateFormat::*;
        match self {
            DdMmYyyy => "DD/MM/YYYY",
            DdMmYy => "DD/MM/YY",
            DdMmYyyyDash => "DD-MM-YYYY",
            DdMmmYyyy => "DD MMM YYYY",
            MmDdYyyy => "MM/DD/YYYY",
            MmDdYy => "MM/DD/YY",
            MmDdYyyyDash => "MM-DD-YYYY",
            MmmDdYyyy => "MMM DD YYYY",
            MmmmDYyyy => "MMMM D, YYYY",
            YyyyMmDd => "YYYY/MM/DD",
            YyyyMmDdDash => "YYYY-MM-DD",
            DdMm => "DD/MM",
            MmDd => "MM/DD",
            DdMmm => "DD MMM",
            MmmDd => "MMM DD",
        }
    }

    fn chrono(self) -> &'static str {
        use DateFormat::*;
        match self {
            DdMmYyyy => "%d/%m/%Y",
            DdMmYy => "%d/%m/%y",
            DdMmYyyyDash => "%d-%m-%Y",
            DdMmmYyyy => "%d %b %Y",
            MmDdYyyy => "%m/%d/%Y",
            MmDdYy => "%m/%d/%y",
            MmDdYyyyDash => "%m-%d-%Y",
            MmmDdYyyy => "%b %d %Y",
            MmmmDYyyy => "%B %-d, %Y",
            YyyyMmDd => "%Y/%m/%d",
            YyyyMmDdDash => "%Y-%m-%d",
            DdMm => "%d/%m",
            MmDd => "%m/%d",
            DdMmm => "%d %b",
            MmmDd => "%b %d",
        }
    }

    pub fn format(self, d: NaiveDate) -> String {
        d.format(self.chrono()).to_string()
    }

    /// Whether the month is written before the day.
    pub fn month_first(self) -> bool {
        use DateFormat::*;
        matches!(self, MmDdYyyy | MmDdYy | MmDdYyyyDash | MmmDdYyyy | MmmmDYyyy | MmDd | MmmDd)
    }
}

/// "20 November 2026", or "November 20, 2026" for month-first formats.
pub fn long(d: NaiveDate, f: DateFormat) -> String {
    d.format(if f.month_first() { "%B %-d, %Y" } else { "%-d %B %Y" }).to_string()
}

/// "Friday, 20 November 2026", or "Friday, November 20, 2026".
pub fn long_with_weekday(d: NaiveDate, f: DateFormat) -> String {
    format!("{}, {}", d.format("%A"), long(d, f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let d = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        let got: Vec<String> = DateFormat::ALL.iter().map(|f| f.format(d)).collect();
        assert_eq!(
            got,
            [
                "05/03/2026",
                "05/03/26",
                "05-03-2026",
                "05 Mar 2026",
                "03/05/2026",
                "03/05/26",
                "03-05-2026",
                "Mar 05 2026",
                "March 5, 2026",
                "2026/03/05",
                "2026-03-05",
                "05/03",
                "03/05",
                "05 Mar",
                "Mar 05",
            ]
        );
        assert_eq!(DateFormat::default(), DateFormat::DdMmm);
        assert_eq!(long_with_weekday(d, DateFormat::DdMmm), "Thursday, 5 March 2026");
        assert_eq!(long_with_weekday(d, DateFormat::MmDd), "Thursday, March 5, 2026");
    }
}
