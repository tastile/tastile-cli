//! Convenience schedule input for `tastile source-tiles create`.
//!
//! The pinned contract (`SourceGenerationSchema`) documents exactly two
//! generation kinds: `0 = one-time` and `1 = recurring`. A recurring
//! definition carries `starts_at`, `interval_ms`, an optional
//! `weekday_mask`, an optional `offset_min` (local calendar offset in
//! minutes east of UTC) and `excluded_dates`.
//!
//! The CLI therefore offers the shapes that map onto those two kinds and
//! refuses everything it cannot express exactly — in particular a monthly
//! recurrence is **not** approximated as a daily one.

use anyhow::{Result, bail};
use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use serde_json::{Value, json};

/// Milliseconds in a minute.
pub const MINUTE_MS: i64 = 60_000;

/// Milliseconds in a day.
pub const DAY_MS: i64 = 24 * 60 * MINUTE_MS;

/// Default placement window: the occurrence may start up to four hours
/// after its nominal time. The contract requires
/// `start_offset_ms < end_offset_ms`.
pub const DEFAULT_WINDOW_END_OFFSET_MS: i64 = 4 * 60 * MINUTE_MS;

/// Days of the week as the contract's `weekday_mask` encodes them: bit 0 is
/// Monday through bit 6 is Sunday.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Weekday {
    Mon = 1,
    Tue = 2,
    Wed = 4,
    Thu = 8,
    Fri = 16,
    Sat = 32,
    Sun = 64,
}

impl Weekday {
    /// Parse an English short day name (`mon`, `tue`, …).
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "mon" | "monday" => Ok(Self::Mon),
            "tue" | "tues" | "tuesday" => Ok(Self::Tue),
            "wed" | "wednesday" => Ok(Self::Wed),
            "thu" | "thur" | "thurs" | "thursday" => Ok(Self::Thu),
            "fri" | "friday" => Ok(Self::Fri),
            "sat" | "saturday" => Ok(Self::Sat),
            "sun" | "sunday" => Ok(Self::Sun),
            other => bail!("unknown weekday `{other}`; expected mon|tue|wed|thu|fri|sat|sun"),
        }
    }

    /// The contract's mask bit for this weekday.
    pub fn bit(self) -> i32 {
        self as i32
    }
}

/// The recurrence shapes the CLI can express exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleInput {
    /// A single occurrence at an exact instant (`kind = 0`).
    OneTime { at: DateTime<Utc> },
    /// Every day at a fixed local wall time (`kind = 1`).
    Daily { anchor: DateTime<Utc> },
    /// The selected weekdays at a fixed local wall time (`kind = 1` with a
    /// `weekday_mask`).
    Weekly {
        anchor: DateTime<Utc>,
        weekdays: Vec<Weekday>,
    },
}

/// A generation object plus a human-readable summary of what it means.
#[derive(Debug, Clone, PartialEq)]
pub struct GenerationPlan {
    pub generation: Value,
    pub summary: String,
}

/// Parse a `HH:MM` clock time.
pub fn parse_clock(raw: &str) -> Result<NaiveTime> {
    let parsed = NaiveTime::parse_from_str(raw.trim(), "%H:%M")
        .map_err(|_| anyhow::anyhow!("invalid time `{raw}`; expected HH:MM (24h, local time)"))?;
    Ok(parsed)
}

/// Parse a comma-separated list of weekday names.
pub fn parse_weekdays(raw: &str) -> Result<Vec<Weekday>> {
    let mut days: Vec<Weekday> = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let day = Weekday::parse(part)?;
        if !days.contains(&day) {
            days.push(day);
        }
    }
    if days.is_empty() {
        bail!("no weekday selected; pass --days like `mon,wed,fri`");
    }
    days.sort();
    Ok(days)
}

/// Convert a local calendar date and wall time into a UTC instant using a
/// fixed offset in minutes east of UTC.
pub fn local_instant(date: NaiveDate, time: NaiveTime, offset_min: i32) -> Result<DateTime<Utc>> {
    let local = date.and_time(time);
    Utc.from_utc_datetime(&local)
        .checked_sub_signed(chrono::Duration::minutes(i64::from(offset_min)))
        .ok_or_else(|| anyhow::anyhow!("local date and UTC offset exceed the representable range"))
}

/// Build the contract-shaped `generation` object for a recurrence.
///
/// `excluded_dates` are ISO-8601 calendar dates evaluated in the same local
/// offset as the recurrence.
pub fn build_generation(
    input: &ScheduleInput,
    excluded_dates: &[String],
    offset_min: Option<i32>,
) -> GenerationPlan {
    let mut generation = json!({});
    match input {
        ScheduleInput::OneTime { at } => {
            generation["kind"] = json!(0);
            generation["at"] = json!(at.to_rfc3339());
        }
        ScheduleInput::Daily { anchor } => {
            generation["kind"] = json!(1);
            generation["starts_at"] = json!(anchor.to_rfc3339());
            generation["interval_ms"] = json!(DAY_MS);
        }
        ScheduleInput::Weekly { anchor, weekdays } => {
            let mask: i32 = weekdays.iter().map(|d| d.bit()).sum();
            generation["kind"] = json!(1);
            generation["starts_at"] = json!(anchor.to_rfc3339());
            generation["interval_ms"] = json!(DAY_MS);
            generation["weekday_mask"] = json!(mask);
        }
    }
    if let Some(offset) = offset_min {
        generation["offset_min"] = json!(offset);
    }
    if !excluded_dates.is_empty() {
        generation["excluded_dates"] = json!(excluded_dates);
    }
    let summary = match input {
        ScheduleInput::OneTime { at } => format!("one-time at {}", at.to_rfc3339()),
        ScheduleInput::Daily { anchor } => {
            format!("daily at {} (fixed UTC offset)", anchor.to_rfc3339())
        }
        ScheduleInput::Weekly { anchor, weekdays } => {
            let names: Vec<&str> = weekdays.iter().map(|d| day_name(*d)).collect();
            format!(
                "weekly on {} at {} (fixed UTC offset)",
                names.join(","),
                anchor.to_rfc3339()
            )
        }
    };
    GenerationPlan {
        generation,
        summary,
    }
}

/// The default placement window: `[0, 4h)` after the nominal time.
pub fn default_window() -> Value {
    json!({
        "start_offset_ms": 0,
        "end_offset_ms": DEFAULT_WINDOW_END_OFFSET_MS,
    })
}

/// Build a placement window from a duration in minutes.
pub fn window_from_minutes(minutes: i64) -> Result<Value> {
    if minutes <= 0 {
        bail!("window minutes must be positive");
    }
    let end_offset_ms = minutes
        .checked_mul(MINUTE_MS)
        .ok_or_else(|| anyhow::anyhow!("window minutes are too large"))?;
    Ok(json!({
        "start_offset_ms": 0,
        "end_offset_ms": end_offset_ms,
    }))
}

fn day_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "mon",
        Weekday::Tue => "tue",
        Weekday::Wed => "wed",
        Weekday::Thu => "thu",
        Weekday::Fri => "fri",
        Weekday::Sat => "sat",
        Weekday::Sun => "sun",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(raw: &str) -> NaiveDate {
        NaiveDate::parse_from_str(raw, "%Y-%m-%d").expect("date")
    }

    #[test]
    fn one_time_generation_uses_kind_zero_and_at() {
        let at = DateTime::parse_from_rfc3339("2026-11-20T09:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let plan = build_generation(&ScheduleInput::OneTime { at }, &[], None);
        assert_eq!(plan.generation["kind"], 0);
        assert_eq!(plan.generation["at"], "2026-11-20T09:00:00+00:00");
        assert!(plan.generation.get("starts_at").is_none());
    }

    #[test]
    fn daily_generation_uses_kind_one_and_a_day_interval() {
        let anchor = DateTime::parse_from_rfc3339("2026-11-20T09:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let plan = build_generation(&ScheduleInput::Daily { anchor }, &[], Some(540));
        assert_eq!(plan.generation["kind"], 1);
        assert_eq!(plan.generation["interval_ms"], DAY_MS);
        assert_eq!(plan.generation["offset_min"], 540);
    }

    #[test]
    fn generation_preserves_the_openapi_int32_utc_offset() {
        let anchor = DateTime::parse_from_rfc3339("2026-11-20T09:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let plan = build_generation(&ScheduleInput::Daily { anchor }, &[], Some(32_768));
        assert_eq!(plan.generation["offset_min"], 32_768);
    }

    #[test]
    fn weekly_generation_encodes_the_weekday_mask() {
        let anchor = DateTime::parse_from_rfc3339("2026-11-20T09:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let days = parse_weekdays("mon,wed,fri").unwrap();
        let plan = build_generation(
            &ScheduleInput::Weekly {
                anchor,
                weekdays: days,
            },
            &[],
            None,
        );
        assert_eq!(plan.generation["weekday_mask"], 1 | 4 | 16);
        assert_eq!(plan.generation["interval_ms"], DAY_MS);
    }

    #[test]
    fn excluded_dates_are_forwarded_verbatim() {
        let anchor = DateTime::parse_from_rfc3339("2026-11-20T09:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let plan = build_generation(
            &ScheduleInput::Daily { anchor },
            &["2026-12-25".to_string()],
            None,
        );
        assert_eq!(plan.generation["excluded_dates"], json!(["2026-12-25"]));
    }

    #[test]
    fn local_instant_applies_the_fixed_offset() {
        let instant =
            local_instant(date("2026-11-20"), parse_clock("09:00").unwrap(), 540).unwrap();
        assert_eq!(instant.to_rfc3339(), "2026-11-20T00:00:00+00:00");
    }

    #[test]
    fn invalid_day_names_are_refused() {
        assert!(parse_weekdays("funday").is_err());
        assert!(parse_weekdays("").is_err());
        assert!(parse_clock("25:00").is_err());
    }

    #[test]
    fn default_window_is_a_valid_non_empty_span() {
        let window = default_window();
        assert!(
            window["start_offset_ms"].as_i64().unwrap() < window["end_offset_ms"].as_i64().unwrap()
        );
    }

    #[test]
    fn rejects_a_window_duration_that_overflows_milliseconds() {
        assert!(window_from_minutes(i64::MAX).is_err());
    }
}
