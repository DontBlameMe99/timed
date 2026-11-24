//! Duration parsing and formatting.
//!
//! Units: y (365 days), d, h, m, s. Units may appear in any order and may
//! repeat. A bare number after a unit takes the next smaller unit, so 1h5 is
//! 1 hour 5 minutes and 1s4h is 1 second 4 hours. A lone number means minutes.

use std::fmt::Write;

use crate::math::{quotient, remainder};

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
pub const DAY: i64 = 24 * HOUR;
const YEAR: i64 = 365 * DAY;
const MAX_SECONDS: i64 = 100 * YEAR;

const fn unit_seconds(unit: char) -> Option<i64> {
    match unit {
        'y' => Some(YEAR),
        'd' => Some(DAY),
        'h' => Some(HOUR),
        'm' => Some(MINUTE),
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
    let bytes = input.as_bytes();
    let mut index: usize = 0;
    let mut total: i64 = 0;
    let mut last = None;
    while let Some(&byte) = bytes.get(index) {
        if byte.is_ascii_whitespace() {
            index = index.saturating_add(1);
            continue;
        }
        let value = read_number(bytes, &mut index)?;
        let (unit, next) = match unit_at(bytes, index) {
            Some(found) => found,
            None => (
                match last {
                    Some(previous) => next_unit(previous)?,
                    None => 'm',
                },
                index,
            ),
        };
        index = next;
        total = total.checked_add(value.checked_mul(unit_seconds(unit)?)?)?;
        last = Some(unit);
    }

    if total <= 0 || total > MAX_SECONDS {
        return None;
    }
    Some(total)
}

fn read_number(bytes: &[u8], index: &mut usize) -> Option<i64> {
    let mut value: i64 = 0;
    let mut seen = false;
    while let Some(&byte) = bytes.get(*index) {
        if byte.is_ascii_whitespace() {
            *index = index.saturating_add(1);
        } else if byte.is_ascii_digit() {
            let digit = i64::from(byte.saturating_sub(b'0'));
            value = value.checked_mul(10)?.checked_add(digit)?;
            seen = true;
            *index = index.saturating_add(1);
        } else {
            break;
        }
    }
    seen.then_some(value)
}

fn unit_at(bytes: &[u8], index: usize) -> Option<(char, usize)> {
    let mut at = index;
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at = at.saturating_add(1);
    }
    let byte = *bytes.get(at)?;
    let unit = byte.to_ascii_lowercase() as char;
    unit_seconds(unit).map(|_| (unit, at.saturating_add(1)))
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
    let end = input
        .bytes()
        .position(|byte: u8| byte.is_ascii_whitespace())
        .unwrap_or(input.len());
    if end == input.len() {
        return parse(input).map(|seconds| (seconds, None));
    }
    let rest = input[end..].trim();
    let continues = rest.as_bytes().first().is_some_and(|byte| {
        byte.is_ascii_digit()
            || matches!(
                byte.to_ascii_lowercase(),
                b'y' | b'd' | b'h' | b'm' | b's'
            )
    });
    if continues && let Some(seconds) = parse(input) {
        return Some((seconds, None));
    }
    let seconds = parse(&input[..end])?;
    let message = (!rest.is_empty()).then(|| rest.to_owned());
    Some((seconds, message))
}

#[must_use]
pub fn clock(seconds: i64) -> String {
    let total = seconds.max(0);
    let days = quotient(total, DAY);
    let hours = quotient(remainder(total, DAY), HOUR);
    let minutes = quotient(remainder(total, HOUR), MINUTE);
    let secs = remainder(total, MINUTE);
    let mut out = String::with_capacity(16);
    if days > 0 {
        let _ = write!(out, "{days}d {hours}:{minutes:02}:{secs:02}");
    } else if hours > 0 {
        let _ = write!(out, "{hours}:{minutes:02}:{secs:02}");
    } else {
        let _ = write!(out, "{minutes}:{secs:02}");
    }
    out
}

#[must_use]
pub fn human(seconds: i64) -> String {
    let total = seconds.max(0);
    let years = quotient(total, YEAR);
    let days = quotient(remainder(total, YEAR), DAY);
    let hours = quotient(remainder(total, DAY), HOUR);
    let minutes = quotient(remainder(total, HOUR), MINUTE);
    let secs = remainder(total, MINUTE);

    let mut out = String::with_capacity(24);
    for (value, unit) in [
        (years, 'y'),
        (days, 'd'),
        (hours, 'h'),
        (minutes, 'm'),
        (secs, 's'),
    ] {
        push_part(&mut out, value, unit);
    }
    if out.is_empty() { "0s".to_owned() } else { out }
}

fn push_part(out: &mut String, value: i64, unit: char) {
    if value <= 0 {
        return;
    }
    if !out.is_empty() {
        out.push(' ');
    }
    let _ = write!(out, "{value}{unit}");
}

/// The single unit the bar shows, rounded to that unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Coarse {
    Minutes(i64),
    Hours(i64),
    Days(i64),
    Years(i64),
}

impl Coarse {
    fn of(seconds: i64) -> Self {
        let total = seconds.max(0);
        if total >= DAY {
            return Self::days(total);
        }
        if total >= HOUR {
            return Self::hours(total);
        }
        let minutes = rounded(total, MINUTE).max(1);
        if minutes >= 60 {
            return Self::Hours(1);
        }
        Self::Minutes(minutes)
    }

    fn days(total: i64) -> Self {
        let days = rounded(total, DAY);
        if days >= 365 {
            return Self::Years(rounded(days, 365));
        }
        Self::Days(days)
    }

    fn hours(total: i64) -> Self {
        let hours = rounded(total, HOUR);
        if hours >= 24 {
            return Self::Days(1);
        }
        Self::Hours(hours)
    }

    fn of_ms(milliseconds: i64) -> Self {
        let total = milliseconds.max(0);
        if total > DAY.saturating_sub(1).saturating_mul(1_000) {
            return Self::days_ms(total);
        }
        if total > HOUR.saturating_sub(1).saturating_mul(1_000) {
            return Self::hours_ms(total);
        }
        let minutes = rounded_ms(total, MINUTE).max(1);
        if minutes >= 60 {
            return Self::Hours(1);
        }
        Self::Minutes(minutes)
    }

    /// Days, or years once the rounded count reaches one.
    fn days_ms(total: i64) -> Self {
        let days = rounded_ms(total, DAY);
        if days >= 365 {
            return Self::Years(rounded(days, 365));
        }
        Self::Days(days)
    }

    /// Hours, or a single day once the rounded count reaches one.
    fn hours_ms(total: i64) -> Self {
        let hours = rounded_ms(total, HOUR);
        if hours >= 24 {
            return Self::Days(1);
        }
        Self::Hours(hours)
    }
}

/// Coarse form for the bar: the nearest whole unit, such as 4h, 30m, or 1m.
#[must_use]
pub fn approx(seconds: i64) -> String {
    match Coarse::of(seconds) {
        Coarse::Minutes(value) => format!("{value}m"),
        Coarse::Hours(value) => format!("{value}h"),
        Coarse::Days(value) => format!("{value}d"),
        Coarse::Years(value) => format!("{value}y"),
    }
}

fn rounded(value: i64, divisor: i64) -> i64 {
    quotient(value.saturating_add(quotient(divisor, 2)), divisor)
}

fn rounded_ms(milliseconds: i64, unit_seconds: i64) -> i64 {
    let unit = unit_seconds.saturating_mul(1_000);
    quotient(
        milliseconds
            .saturating_add(999)
            .saturating_add(quotient(unit, 2)),
        unit,
    )
}

/// Allocation-free identity of the string `approx` would return. Render
/// caches use it to tell when the coarse countdown actually moves.
#[must_use]
pub fn approx_token(seconds: i64) -> u64 {
    coarse_token(Coarse::of(seconds))
}

/// Allocation-free identity of `approx` for a remaining millisecond value.
#[must_use]
pub fn approx_token_ms(milliseconds: i64) -> u64 {
    coarse_token(Coarse::of_ms(milliseconds))
}

fn coarse_token(coarse: Coarse) -> u64 {
    match coarse {
        Coarse::Years(value) => 1_000_000_u64.saturating_add(as_u64(value)),
        Coarse::Days(value) => 2_000_000_u64.saturating_add(as_u64(value)),
        Coarse::Hours(value) => 4_000_000_u64.saturating_add(as_u64(value)),
        Coarse::Minutes(value) => 6_000_000_u64.saturating_add(as_u64(value)),
    }
}

fn as_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
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
        assert_eq!(approx(3_599), "1h");
        assert_eq!(approx(3_600), "1h");
        assert_eq!(approx(86_399), "1d");
        assert_eq!(approx(90_000), "1d");
        assert_eq!(approx(400 * DAY), "1y");
    }

    #[test]
    fn approx_token_tells_apart_what_approx_shows() {
        let token = |seconds| approx_token(seconds);
        assert_ne!(token(0), token(90));
        assert_eq!(token(90), token(120));
        assert_ne!(token(3_000), token(3_600));
        assert_eq!(token(86_399), token(86_400));
        assert_ne!(token(90_000), token(400 * DAY));
    }
    fn reference_split(text: &str) -> Option<(i64, Option<String>)> {
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
        let seconds = parse(&input[..end])?;
        let rest = input[end..].trim();
        let message = (!rest.is_empty()).then(|| rest.to_owned());
        Some((seconds, message))
    }

    #[test]
    #[expect(
        clippy::excessive_nesting,
        reason = "the test enumerates short combinations of duration tokens"
    )]
    fn split_matches_the_whole_text_first_rule() {
        let pieces = ["1", "5", "h", "m", "s", "x", " ", "  "];
        let mut cases = Vec::new();
        for first in pieces {
            for second in pieces {
                for third in pieces {
                    for fourth in pieces {
                        cases.push(format!("{first}{second}{third}{fourth}"));
                    }
                }
            }
        }
        for text in cases {
            assert_eq!(split(&text), reference_split(&text), "{text}");
        }
    }

    #[expect(
        clippy::excessive_nesting,
        reason = "the test checks every millisecond around each unit boundary"
    )]
    #[test]
    fn millisecond_tokens_match_rounded_seconds() {
        for seconds in [
            0,
            1,
            29,
            30,
            59,
            60,
            3_569,
            3_570,
            3_599,
            3_600,
            HOUR.saturating_mul(24).saturating_sub(1),
            HOUR.saturating_mul(24),
            DAY.saturating_mul(365).saturating_sub(1),
            DAY.saturating_mul(365),
            MAX_SECONDS,
        ] {
            for offset in 0..1_000 {
                let milliseconds =
                    seconds.saturating_mul(1_000).saturating_add(offset);
                let rounded_seconds =
                    seconds.saturating_add(i64::from(offset != 0));
                assert_eq!(
                    approx_token_ms(milliseconds),
                    approx_token(rounded_seconds)
                );
            }
        }
        for milliseconds in [
            0,
            1,
            29_999,
            30_000,
            30_001,
            3_569_999,
            3_570_000,
            HOUR.saturating_mul(1_000).saturating_sub(1),
            HOUR.saturating_mul(1_000),
            DAY.saturating_mul(1_000).saturating_sub(1),
            DAY.saturating_mul(1_000),
        ] {
            let seconds = quotient(milliseconds, 1_000)
                .saturating_add(i64::from(remainder(milliseconds, 1_000) != 0));
            assert_eq!(approx_token_ms(milliseconds), approx_token(seconds));
        }
    }
}
