//! Minimal RFC 3339 handling for the IRCv3 `time` tag, in UTC only.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Parse `YYYY-MM-DDThh:mm:ss[.fff]Z`.
pub fn parse(s: &str) -> Option<SystemTime> {
    let b = s.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<u32>().ok();
    let (y, mo, d) = (num(0..4)? as i64, num(5..7)?, num(8..10)?);
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let rest = &s[19..];
    let (frac, tail) = match rest.strip_prefix('.') {
        Some(r) => {
            let end = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
            (&r[..end], &r[end..])
        }
        None => ("", rest),
    };
    if tail != "Z" {
        return None;
    }
    let nanos = if frac.is_empty() {
        0
    } else {
        let digits: String = frac.chars().chain(std::iter::repeat('0')).take(9).collect();
        digits.parse::<u32>().ok()?
    };
    let secs = days_from_civil(y, mo, d) * 86_400 + (h * 3600 + mi * 60 + sec) as i64;
    let secs = u64::try_from(secs).ok()?;
    Some(UNIX_EPOCH + Duration::new(secs, nanos))
}

pub fn format(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (y, m, day) = civil_from_days(secs.div_euclid(86_400));
    let sod = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        sod / 3600,
        (sod / 60) % 60,
        sod % 60,
        d.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = "2026-11-10T13:45:07.123Z";
        assert_eq!(format(parse(s).unwrap()), s);
        assert_eq!(
            format(parse("2000-02-29T00:00:00Z").unwrap()),
            "2000-02-29T00:00:00.000Z"
        );
        assert!(parse("2026-11-10 13:45:07Z").is_none());
        assert!(parse("2026-11-10T13:45:07+01:00").is_none());
    }
}
