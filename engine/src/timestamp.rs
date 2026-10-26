//! UTC instants as Unix milliseconds and their ISO 8601 text form.

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    era * 146_097 + of_cycle - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let of_era = days - era * 146_097;
    let of_cycle = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * of_cycle + of_cycle / 4 - of_cycle / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = of_cycle + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `Date.prototype.toISOString` for instants in years 0000–9999.
pub fn format(ms: i64) -> String {
    let (days, within) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        within / 3_600_000,
        within / 60_000 % 60,
        within / 1000 % 60,
        within % 1000
    )
}

fn number(text: &str, digits: usize) -> Option<i64> {
    (text.len() == digits && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// Strict ISO 8601: a UTC date, or a date-time with seconds optional, fraction
/// optional and an explicit `Z` or `±HH:MM` offset. Local date-times and the
/// free-form text `Date.parse` also accepts are rejected.
pub fn parse(text: &str) -> Option<i64> {
    let (date, time) = match text.split_once('T') {
        Some((date, time)) => (date, Some(time)),
        None => (text, None),
    };
    let mut parts = date.split('-');
    let year = number(parts.next()?, 4)?;
    let month = number(parts.next()?, 2)?;
    let day = number(parts.next()?, 2)?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let length = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if parts.next().is_some()
        || !(1..=12).contains(&month)
        || day < 1
        || day > length[month as usize - 1]
    {
        return None;
    }
    let mut ms = days_from_civil(year, month, day) * 86_400_000;
    let Some(time) = time else { return Some(ms) };
    let (clock, offset) = if let Some(clock) = time.strip_suffix('Z') {
        (clock, 0)
    } else {
        let at = time.rfind(['+', '-'])?;
        let (clock, zone) = time.split_at(at);
        let (hours, minutes) = zone[1..].split_once(':')?;
        let (hours, minutes) = (number(hours, 2)?, number(minutes, 2)?);
        if hours > 23 || minutes > 59 {
            return None;
        }
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        (clock, sign * (hours * 60 + minutes) * 60_000)
    };
    let (clock, fraction) = match clock.split_once('.') {
        Some((clock, fraction))
            if !fraction.is_empty() && fraction.bytes().all(|b| b.is_ascii_digit()) =>
        {
            (
                clock,
                fraction[..fraction.len().min(3)].parse::<i64>().ok()?
                    * 10_i64.pow(3 - fraction.len().min(3) as u32),
            )
        }
        Some(_) => return None,
        None => (clock, 0),
    };
    let mut fields = clock.split(':');
    let hours = number(fields.next()?, 2)?;
    let minutes = number(fields.next()?, 2)?;
    let seconds = fields.next().map_or(Some(0), |s| number(s, 2))?;
    if fields.next().is_some()
        || hours > 24
        || minutes > 59
        || seconds > 59
        || (hours == 24 && (minutes, seconds, fraction) != (0, 0, 0))
    {
        return None;
    }
    ms += ((hours * 60 + minutes) * 60 + seconds) * 1000 + fraction - offset;
    Some(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_text_round_trips_and_rejects_ambiguous_forms() {
        for (ms, text) in [
            (0, "1970-01-01T00:00:00.000Z"),
            (951_782_400_000, "2000-02-29T00:00:00.000Z"),
            (1_793_030_973_650, "2026-10-26T16:09:33.650Z"),
            (-1, "1969-12-31T23:59:59.999Z"),
        ] {
            assert_eq!(format(ms), text);
            assert_eq!(parse(text), Some(ms));
        }
        assert_eq!(parse("2026-10-24"), parse("2026-10-24T00:00:00Z"));
        assert_eq!(
            parse("2026-10-24T02:00+02:00"),
            parse("2026-10-24T00:00:00.000Z")
        );
        assert_eq!(
            parse("2026-10-24T00:00:00.1234Z"),
            parse("2026-10-24T00:00:00.123Z")
        );
        for text in [
            "",
            "2026-10-24T00:00:00",
            "2026-02-30",
            "2026-10-24T25:00Z",
            "Oct 24 2026",
            "123",
            "2026-10-24T00:00:00.Z",
            "2026-10-24T00:00:00+0200",
            "+02026-10-24",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
    }
}
