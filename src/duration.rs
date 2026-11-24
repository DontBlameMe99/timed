//! Duration parsing and formatting.
//!
//! Units: y (365 days), d, h, m, s. Units may appear in any order and may
//! repeat. A bare number after a unit takes the next smaller unit, so 1h5 is
//! 1 hour 5 minutes and 1s4h is 1 second 4 hours. A lone number means minutes.

use crate::math::{quotient, remainder};

const YEAR: i64 = 31_536_000;
const MAX_SECONDS: i64 = 100 * YEAR;

const fn unit_seconds(unit: char) -> Option<i64> {
    match unit {
        'y' => Some(YEAR),
        'd' => Some(86_400),
        'h' => Some(3_600),
        'm' => Some(60),
        's' => Some(1),
        _ => None,
    }
}

const fn next_unit(unit: char) -> Option<char> {
    match unit {
        'y' => Some('d'),
        'd' => Some('h'),
        'h' => Some('m'),
        'm' => Some('s'),
        _ => None,
    }
}

#[must_use]
pub fn parse(input: &str) -> Option<i64> {
    let text: String = input
        .to_ascii_lowercase()
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect();
    if text.is_empty() {
        return None;
    }

    let bytes = text.as_bytes();
    let mut index = 0;
    let mut total: i64 = 0;
    let mut last: Option<char> = None;

    while index < bytes.len() {
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index = index.saturating_add(1);
        }
        if start == index {
            return None;
        }
        let value: i64 = text.get(start..index)?.parse().ok()?;

        let unit = match bytes.get(index).copied().map(char::from) {
            Some(character) if unit_seconds(character).is_some() => {
                index = index.saturating_add(1);
                character
            }
            _ => match last {
                Some(previous) => next_unit(previous)?,
                None => 'm',
            },
        };

        total = total.checked_add(value.checked_mul(unit_seconds(unit)?)?)?;
        last = Some(unit);
    }

    if total <= 0 || total > MAX_SECONDS {
        return None;
    }
    Some(total)
}

/// Splits launcher or IPC text into a duration and an optional message.
/// The whole text wins when it parses as a duration, so spaced formats still
/// work. Otherwise the first word is the duration and the rest is the message.
#[must_use]
pub fn split(text: &str) -> Option<(i64, Option<String>)> {
    let input = text.trim();
    if input.is_empty() {
        return None;
    }
    if let Some(seconds) = parse(input) {
        return Some((seconds, None));
    }

    let end = input
        .find(|character: char| character.is_ascii_whitespace())
        .unwrap_or(input.len());
    let seconds = parse(input.get(..end)?)?;
    let rest = input.get(end..).unwrap_or_default().trim();
    let message = (!rest.is_empty()).then(|| rest.to_owned());
    Some((seconds, message))
}

#[must_use]
pub fn clock(seconds: i64) -> String {
    let total = seconds.max(0);
    let days = quotient(total, 86_400);
    let hours = quotient(remainder(total, 86_400), 3_600);
    let minutes = quotient(remainder(total, 3_600), 60);
    let secs = remainder(total, 60);
    if days > 0 {
        format!("{days}d {hours}:{minutes:02}:{secs:02}")
    } else if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

#[must_use]
pub fn human(seconds: i64) -> String {
    let total = seconds.max(0);
    let years = quotient(total, YEAR);
    let days = quotient(remainder(total, YEAR), 86_400);
    let hours = quotient(remainder(total, 86_400), 3_600);
    let minutes = quotient(remainder(total, 3_600), 60);
    let secs = remainder(total, 60);

    let mut parts = Vec::new();
    for (value, unit) in [
        (years, 'y'),
        (days, 'd'),
        (hours, 'h'),
        (minutes, 'm'),
        (secs, 's'),
    ] {
        if value > 0 {
            parts.push(format!("{value}{unit}"));
        }
    }
    if parts.is_empty() {
        return "0s".to_owned();
    }
    parts.join(" ")
}

/// Coarse form for the bar: the nearest whole unit, such as 4h, 30m, or 1m.
#[must_use]
pub fn approx(seconds: i64) -> String {
    let total = seconds.max(0);
    if total < 60 {
        return "1m".to_owned();
    }
    if total >= 86_400 {
        let days = rounded(total, 86_400);
        if days >= 365 {
            return format!("{}y", rounded(days, 365));
        }
        return format!("{days}d");
    }
    if total >= 3_600 {
        let hours = rounded(total, 3_600);
        if hours >= 24 {
            return "1d".to_owned();
        }
        return format!("{hours}h");
    }
    let minutes = rounded(total, 60);
    if minutes >= 60 {
        "1h".to_owned()
    } else {
        format!("{minutes}m")
    }
}

fn rounded(value: i64, divisor: i64) -> i64 {
    quotient(value.saturating_add(quotient(divisor, 2)), divisor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_documented_forms() {
        assert_eq!(parse("5"), Some(300));
        assert_eq!(parse("1h5"), Some(3_900));
        assert_eq!(parse("1m5"), Some(65));
        assert_eq!(parse("3h20m10s"), Some(12_010));
        assert_eq!(parse("1s4h"), Some(14_401));
        assert_eq!(parse("4s10h3m"), Some(36_184));
        assert_eq!(parse("1h 5m"), Some(3_900));
        assert_eq!(parse("1y"), Some(YEAR));
        assert_eq!(parse("1y2y"), Some(94_608_000));
        assert_eq!(parse("100y"), Some(3_153_600_000));
    }

    #[test]
    fn parse_rejects_bad_input() {
        for input in ["", "abc", "0", "1x", "1.5h", "1s5", "101y"] {
            assert_eq!(parse(input), None, "{input}");
        }
    }

    #[test]
    fn split_separates_duration_from_message() {
        assert_eq!(split("1h 5m"), Some((3_900, None)));
        assert_eq!(
            split("3h2s something I want"),
            Some((10_802, Some("something I want".to_owned())))
        );
        assert_eq!(
            split("30 walk the dog"),
            Some((1_800, Some("walk the dog".to_owned())))
        );
        assert_eq!(split("1h5 30"), Some((35_400, None)));
        assert_eq!(split(""), None);
        assert_eq!(split("nope"), None);
    }

    #[test]
    fn clock_formats_largest_unit() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(65), "1:05");
        assert_eq!(clock(3_900), "1:05:00");
        assert_eq!(clock(90_061), "1d 1:01:01");
        assert_eq!(clock(-5), "0:00");
    }

    #[test]
    fn human_lists_each_unit() {
        assert_eq!(human(0), "0s");
        assert_eq!(human(3_900), "1h 5m");
        assert_eq!(human(YEAR), "1y");
        assert_eq!(human(90_061), "1d 1h 1m 1s");
        assert_eq!(human(YEAR + 90_061), "1y 1d 1h 1m 1s");
    }

    #[test]
    fn approx_rounds_to_nearest_unit() {
        assert_eq!(approx(0), "1m");
        assert_eq!(approx(59), "1m");
        assert_eq!(approx(90), "2m");
        assert_eq!(approx(3_600), "1h");
        assert_eq!(approx(90_000), "1d");
        assert_eq!(approx(400 * 86_400), "1y");
    }
}
