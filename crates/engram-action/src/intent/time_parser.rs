//! Natural language time expression parsing.
//!
//! Extracts temporal references (e.g., "in 5 minutes", "at 3pm", "tomorrow")
//! from intent text and converts them to `Timestamp` values.

use chrono::{Datelike, Duration, Local, NaiveTime, Weekday};
use engram_core::types::Timestamp;
use regex::Regex;

/// Rule-based time expression parser.
///
/// Tries patterns in order and returns the first match.
pub struct TimeExpressionParser;

impl TimeExpressionParser {
    /// Parse a time expression from the given text.
    ///
    /// Returns `None` if no recognized time expression is found.
    pub fn parse(text: &str) -> Option<Timestamp> {
        let lower = text.to_lowercase();

        // Try patterns in order of specificity
        Self::parse_in_minutes(&lower)
            .or_else(|| Self::parse_in_hours(&lower))
            .or_else(|| Self::parse_at_time(&lower))
            .or_else(|| Self::parse_tomorrow(&lower))
            .or_else(|| Self::parse_next_weekday(&lower))
            .or_else(|| Self::parse_on_date(&lower))
    }

    /// "in N minutes/mins" -> now + N minutes
    fn parse_in_minutes(text: &str) -> Option<Timestamp> {
        let re = Regex::new(r"\bin\s+(\d+)\s+(?:minutes?|mins?)\b").ok()?;
        let caps = re.captures(text)?;
        let n: i64 = caps.get(1)?.as_str().parse().ok()?;
        let target = Local::now() + Duration::minutes(n);
        Some(Timestamp(target.timestamp()))
    }

    /// "in N hours/hour" or "in an hour" -> now + N hours
    fn parse_in_hours(text: &str) -> Option<Timestamp> {
        // "in an hour"
        let re_an = Regex::new(r"\bin\s+an?\s+hour\b").ok()?;
        if re_an.is_match(text) {
            let target = Local::now() + Duration::hours(1);
            return Some(Timestamp(target.timestamp()));
        }

        let re = Regex::new(r"\bin\s+(\d+)\s+(?:hours?|hrs?)\b").ok()?;
        let caps = re.captures(text)?;
        let n: i64 = caps.get(1)?.as_str().parse().ok()?;
        let target = Local::now() + Duration::hours(n);
        Some(Timestamp(target.timestamp()))
    }

    /// "at 3pm", "at 15:00", "at 3" -> today/tomorrow at that time
    fn parse_at_time(text: &str) -> Option<Timestamp> {
        // "at H:MM am/pm" or "at H:MM"
        let re_hm = Regex::new(r"\bat\s+(\d{1,2}):(\d{2})\s*(am|pm)?\b").ok()?;
        if let Some(caps) = re_hm.captures(text) {
            let mut hour: u32 = caps.get(1)?.as_str().parse().ok()?;
            let min: u32 = caps.get(2)?.as_str().parse().ok()?;

            if let Some(ampm) = caps.get(3) {
                match ampm.as_str() {
                    "pm" if hour < 12 => hour += 12,
                    "am" if hour == 12 => hour = 0,
                    _ => {}
                }
            }

            return Self::resolve_time_today_or_tomorrow(hour, min);
        }

        // "at Hpm" or "at Ham"
        let re_h_ampm = Regex::new(r"\bat\s+(\d{1,2})\s*(am|pm)\b").ok()?;
        if let Some(caps) = re_h_ampm.captures(text) {
            let mut hour: u32 = caps.get(1)?.as_str().parse().ok()?;
            let ampm = caps.get(2)?.as_str();
            match ampm {
                "pm" if hour < 12 => hour += 12,
                "am" if hour == 12 => hour = 0,
                _ => {}
            }
            return Self::resolve_time_today_or_tomorrow(hour, 0);
        }

        // "at H" (bare hour, assume 24h or daytime)
        let re_bare = Regex::new(r"\bat\s+(\d{1,2})\b").ok()?;
        if let Some(caps) = re_bare.captures(text) {
            let hour: u32 = caps.get(1)?.as_str().parse().ok()?;
            if hour <= 23 {
                return Self::resolve_time_today_or_tomorrow(hour, 0);
            }
        }

        None
    }

    /// "tomorrow" -> tomorrow at 09:00
    fn parse_tomorrow(text: &str) -> Option<Timestamp> {
        let re = Regex::new(r"\btomorrow\b").ok()?;
        if re.is_match(text) {
            let tomorrow = Local::now().date_naive() + Duration::days(1);
            let dt = tomorrow.and_time(NaiveTime::from_hms_opt(9, 0, 0)?);
            let local_dt = dt.and_local_timezone(Local).single()?;
            return Some(Timestamp(local_dt.timestamp()));
        }
        None
    }

    /// "next Monday/Tuesday/..." -> next occurrence at 09:00
    fn parse_next_weekday(text: &str) -> Option<Timestamp> {
        let re = Regex::new(r"\bnext\s+(monday|tuesday|wednesday|thursday|friday|saturday|sunday)\b")
            .ok()?;
        let caps = re.captures(text)?;
        let day_str = caps.get(1)?.as_str();
        let target_weekday = match day_str {
            "monday" => Weekday::Mon,
            "tuesday" => Weekday::Tue,
            "wednesday" => Weekday::Wed,
            "thursday" => Weekday::Thu,
            "friday" => Weekday::Fri,
            "saturday" => Weekday::Sat,
            "sunday" => Weekday::Sun,
            _ => return None,
        };

        let today = Local::now().date_naive();
        let current_weekday = today.weekday();
        let days_ahead = (target_weekday.num_days_from_monday() as i64
            - current_weekday.num_days_from_monday() as i64
            + 7)
            % 7;
        // If it's the same day, go to next week
        let days_ahead = if days_ahead == 0 { 7 } else { days_ahead };

        let target_date = today + Duration::days(days_ahead);
        let dt = target_date.and_time(NaiveTime::from_hms_opt(9, 0, 0)?);
        let local_dt = dt.and_local_timezone(Local).single()?;
        Some(Timestamp(local_dt.timestamp()))
    }

    /// "on February 20th", "on Feb 20" -> specified date at 09:00
    fn parse_on_date(text: &str) -> Option<Timestamp> {
        let re = Regex::new(
            r"\bon\s+(january|february|march|april|may|june|july|august|september|october|november|december|jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)\s+(\d{1,2})(?:st|nd|rd|th)?\b",
        )
        .ok()?;
        let caps = re.captures(text)?;
        let month_str = caps.get(1)?.as_str();
        let day: u32 = caps.get(2)?.as_str().parse().ok()?;

        let month = match month_str {
            "january" | "jan" => 1,
            "february" | "feb" => 2,
            "march" | "mar" => 3,
            "april" | "apr" => 4,
            "may" => 5,
            "june" | "jun" => 6,
            "july" | "jul" => 7,
            "august" | "aug" => 8,
            "september" | "sep" => 9,
            "october" | "oct" => 10,
            "november" | "nov" => 11,
            "december" | "dec" => 12,
            _ => return None,
        };

        let today = Local::now().date_naive();
        let year = today.year();

        // Try this year first, if past, try next year
        let mut target = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
        if target < today {
            target = chrono::NaiveDate::from_ymd_opt(year + 1, month, day)?;
        }

        let dt = target.and_time(NaiveTime::from_hms_opt(9, 0, 0)?);
        let local_dt = dt.and_local_timezone(Local).single()?;
        Some(Timestamp(local_dt.timestamp()))
    }

    /// Resolve a time to today if it's in the future, or tomorrow if it's past.
    fn resolve_time_today_or_tomorrow(hour: u32, minute: u32) -> Option<Timestamp> {
        let now = Local::now();
        let today = now.date_naive();
        let target_time = NaiveTime::from_hms_opt(hour, minute, 0)?;
        let target_dt = today.and_time(target_time);
        let local_dt = target_dt.and_local_timezone(Local).single()?;

        if local_dt > now {
            Some(Timestamp(local_dt.timestamp()))
        } else {
            // Past time today, resolve to tomorrow
            let tomorrow = today + Duration::days(1);
            let target_dt = tomorrow.and_time(target_time);
            let local_dt = target_dt.and_local_timezone(Local).single()?;
            Some(Timestamp(local_dt.timestamp()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    // =====================================================================
    // "in N minutes" tests
    // =====================================================================

    #[test]
    fn test_in_5_minutes() {
        let ts = TimeExpressionParser::parse("in 5 minutes").unwrap();
        let now = Timestamp::now().0;
        let diff = ts.0 - now;
        assert!((290..=310).contains(&diff), "Expected ~300s, got {}s diff", diff);
    }

    #[test]
    fn test_in_30_mins() {
        let ts = TimeExpressionParser::parse("in 30 mins").unwrap();
        let now = Timestamp::now().0;
        let diff = ts.0 - now;
        assert!((1790..=1810).contains(&diff), "Expected ~1800s, got {}s diff", diff);
    }

    #[test]
    fn test_in_1_minute() {
        let ts = TimeExpressionParser::parse("in 1 minute").unwrap();
        let now = Timestamp::now().0;
        let diff = ts.0 - now;
        assert!((50..=70).contains(&diff), "Expected ~60s, got {}s diff", diff);
    }

    // =====================================================================
    // "in N hours" tests
    // =====================================================================

    #[test]
    fn test_in_2_hours() {
        let ts = TimeExpressionParser::parse("in 2 hours").unwrap();
        let now = Timestamp::now().0;
        let diff = ts.0 - now;
        assert!((7190..=7210).contains(&diff), "Expected ~7200s, got {}s diff", diff);
    }

    #[test]
    fn test_in_an_hour() {
        let ts = TimeExpressionParser::parse("in an hour").unwrap();
        let now = Timestamp::now().0;
        let diff = ts.0 - now;
        assert!((3590..=3610).contains(&diff), "Expected ~3600s, got {}s diff", diff);
    }

    #[test]
    fn test_in_1_hr() {
        let ts = TimeExpressionParser::parse("in 1 hr").unwrap();
        let now = Timestamp::now().0;
        let diff = ts.0 - now;
        assert!((3590..=3610).contains(&diff), "Expected ~3600s, got {}s diff", diff);
    }

    // =====================================================================
    // "tomorrow" tests
    // =====================================================================

    #[test]
    fn test_tomorrow() {
        let ts = TimeExpressionParser::parse("tomorrow").unwrap();
        let target_dt = ts.to_datetime();
        let now = Local::now();
        let tomorrow = now.date_naive() + Duration::days(1);
        // Should be tomorrow at 09:00
        assert_eq!(target_dt.hour(), 9);
        assert_eq!(target_dt.minute(), 0);
        // Date should be tomorrow (comparing as naive dates to avoid TZ issues)
        let target_naive = target_dt.date_naive();
        assert_eq!(target_naive, tomorrow);
    }

    #[test]
    fn test_tomorrow_embedded() {
        let ts = TimeExpressionParser::parse("remind me tomorrow to call Bob").unwrap();
        assert!(ts.0 > Timestamp::now().0);
    }

    // =====================================================================
    // "at TIME" tests
    // =====================================================================

    #[test]
    fn test_at_3pm() {
        let ts = TimeExpressionParser::parse("at 3pm").unwrap();
        let dt = ts.to_datetime();
        assert_eq!(dt.hour(), 15);
        assert_eq!(dt.minute(), 0);
        assert!(ts.0 > Timestamp::now().0, "Resolved time should be in the future");
    }

    #[test]
    fn test_at_15_00() {
        let ts = TimeExpressionParser::parse("at 15:00").unwrap();
        let dt = ts.to_datetime();
        assert_eq!(dt.hour(), 15);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn test_at_9_30_am() {
        let ts = TimeExpressionParser::parse("at 9:30 am").unwrap();
        let dt = ts.to_datetime();
        assert_eq!(dt.hour(), 9);
        assert_eq!(dt.minute(), 30);
    }

    #[test]
    fn test_at_12am() {
        let ts = TimeExpressionParser::parse("at 12am").unwrap();
        let dt = ts.to_datetime();
        assert_eq!(dt.hour(), 0);
    }

    #[test]
    fn test_at_12pm() {
        let ts = TimeExpressionParser::parse("at 12pm").unwrap();
        let dt = ts.to_datetime();
        assert_eq!(dt.hour(), 12);
    }

    // =====================================================================
    // "next WEEKDAY" tests
    // =====================================================================

    #[test]
    fn test_next_monday() {
        let ts = TimeExpressionParser::parse("next monday").unwrap();
        let dt = ts.to_datetime();
        assert_eq!(dt.weekday(), Weekday::Mon);
        assert!(ts.0 > Timestamp::now().0);
        assert_eq!(dt.hour(), 9);
    }

    #[test]
    fn test_next_friday() {
        let ts = TimeExpressionParser::parse("next friday").unwrap();
        let dt = ts.to_datetime();
        assert_eq!(dt.weekday(), Weekday::Fri);
        assert!(ts.0 > Timestamp::now().0);
    }

    #[test]
    fn test_next_sunday() {
        let ts = TimeExpressionParser::parse("next sunday").unwrap();
        let dt = ts.to_datetime();
        assert_eq!(dt.weekday(), Weekday::Sun);
        assert!(ts.0 > Timestamp::now().0);
    }

    // =====================================================================
    // "on DATE" tests
    // =====================================================================

    #[test]
    fn test_on_february_20th() {
        let ts = TimeExpressionParser::parse("on february 20th").unwrap();
        let dt = ts.to_datetime();
        assert_eq!(dt.month(), 2);
        assert_eq!(dt.day(), 20);
        assert_eq!(dt.hour(), 9);
        assert!(ts.0 > Timestamp::now().0);
    }

    #[test]
    fn test_on_feb_20() {
        let ts = TimeExpressionParser::parse("on feb 20").unwrap();
        let dt = ts.to_datetime();
        assert_eq!(dt.month(), 2);
        assert_eq!(dt.day(), 20);
    }

    #[test]
    fn test_on_december_25th() {
        let ts = TimeExpressionParser::parse("on december 25th").unwrap();
        let dt = ts.to_datetime();
        assert_eq!(dt.month(), 12);
        assert_eq!(dt.day(), 25);
    }

    // =====================================================================
    // Edge cases
    // =====================================================================

    #[test]
    fn test_no_time_expression() {
        assert!(TimeExpressionParser::parse("hello world").is_none());
    }

    #[test]
    fn test_empty_text() {
        assert!(TimeExpressionParser::parse("").is_none());
    }

    #[test]
    fn test_gibberish() {
        assert!(TimeExpressionParser::parse("asdfghjkl").is_none());
    }

    #[test]
    fn test_future_resolution() {
        // "at 3pm" should always resolve to the future
        let ts = TimeExpressionParser::parse("at 3pm").unwrap();
        assert!(ts.0 > Timestamp::now().0);
    }

    #[test]
    fn test_in_0_minutes() {
        let ts = TimeExpressionParser::parse("in 0 minutes").unwrap();
        let now = Timestamp::now().0;
        let diff = (ts.0 - now).abs();
        assert!(diff <= 5, "Expected ~0s diff, got {}s", diff);
    }

    #[test]
    fn test_text_with_embedded_time() {
        let ts = TimeExpressionParser::parse("remind me to call Bob in 10 minutes please");
        assert!(ts.is_some());
        let diff = ts.unwrap().0 - Timestamp::now().0;
        assert!((590..=610).contains(&diff), "Expected ~600s, got {}s", diff);
    }
}
