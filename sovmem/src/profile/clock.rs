use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(String);

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ClockError {
    #[error("timestamp format is invalid")]
    Invalid,
    #[error("system clock is invalid")]
    System,
}

impl Timestamp {
    pub fn parse(value: &str) -> Result<Self, ClockError> {
        validate_timestamp_bytes(value.as_bytes())?;
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn utc_date(&self) -> &str {
        &self.0[..10]
    }
}

#[allow(dead_code)]
pub trait Clock {
    fn now(&self) -> Result<Timestamp, ClockError>;
}

#[allow(dead_code)]
pub(crate) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Result<Timestamp, ClockError> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ClockError::System)?;
        timestamp_from_unix(elapsed.as_secs(), elapsed.subsec_nanos())
    }
}

#[allow(dead_code)]
pub(crate) fn timestamp_from_unix(seconds: u64, nanoseconds: u32) -> Result<Timestamp, ClockError> {
    if nanoseconds >= 1_000_000_000 {
        return Err(ClockError::System);
    }
    let days = seconds / 86_400;
    let time = seconds % 86_400;
    let (year, month, day) = civil_from_days(days)?;
    let hour = time / 3_600;
    let minute = (time % 3_600) / 60;
    let second = time % 60;
    Timestamp::parse(&format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{nanoseconds:09}Z"
    ))
    .map_err(|_| ClockError::System)
}

fn validate_timestamp_bytes(bytes: &[u8]) -> Result<(), ClockError> {
    if bytes.len() != 30
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'.'
        || bytes[29] != b'Z'
        || ![0..4, 5..7, 8..10, 11..13, 14..16, 17..19, 20..29]
            .iter()
            .all(|range| bytes[range.clone()].iter().all(u8::is_ascii_digit))
    {
        return Err(ClockError::Invalid);
    }
    let year = decimal(&bytes[0..4]);
    let month = decimal(&bytes[5..7]);
    let day = decimal(&bytes[8..10]);
    let hour = decimal(&bytes[11..13]);
    let minute = decimal(&bytes[14..16]);
    let second = decimal(&bytes[17..19]);
    if month == 0
        || month > 12
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(ClockError::Invalid);
    }
    Ok(())
}

fn decimal(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0, |value, byte| value * 10 + u32::from(*byte - b'0'))
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: u32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

#[allow(dead_code)]
fn civil_from_days(days: u64) -> Result<(u32, u32, u32), ClockError> {
    let z = i128::from(days) + 719_468;
    let era = z / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    if !(0..=9_999).contains(&year) {
        return Err(ClockError::System);
    }
    Ok((year as u32, month as u32, day as u32))
}

#[cfg(test)]
mod tests {
    use super::{timestamp_from_unix, Timestamp};

    #[test]
    fn formats_epoch_leap_day_and_nanoseconds_exactly() {
        assert_eq!(
            timestamp_from_unix(0, 0).unwrap().as_str(),
            "1970-01-01T00:00:00.000000000Z"
        );
        assert_eq!(
            timestamp_from_unix(1_709_251_199, 7).unwrap().as_str(),
            "2024-02-29T23:59:59.000000007Z"
        );
        assert_eq!(
            Timestamp::parse("2026-12-31T23:59:59.999999999Z")
                .unwrap()
                .utc_date(),
            "2026-12-31"
        );
    }
}
