use crate::processing::event::Event;
use crate::utils::DateFormat;
use chrono::Datelike;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, ToSchema, Serialize, Deserialize)]
pub enum Field {
    Title,
    Description,
    Location,
    StartTime,
    EndTime,
}

#[derive(Clone, Debug, ToSchema, Serialize, Deserialize)]
pub enum MatchType {
    Exact,
    Contains,
    StartsWith,
    EndsWith,
    Regex,
    BetweenDates, // Matches events between two dates
    Weekdays,     // Matches events on specific weekdays (e.g., "Mon,Wed,Fri")
    TimeOfDay,    // Matches events at specific times (e.g., "09:00,17:00")
}

#[derive(Clone, Debug, ToSchema, Serialize, Deserialize)]
pub struct Matcher {
    id: String,
    field: Field,
    match_type: MatchType,
    value: String,
    negated: bool,
}

impl Matcher {
    pub fn matches(&self, event: &Event) -> bool {
        let bool = match self.field {
            Field::Title => self.matches_string(&event.summary),
            Field::Description => self.matches_string(&event.description),
            Field::Location => self.matches_string(&event.location),
            Field::StartTime => self.matches_date(&event.start),
            Field::EndTime => self.matches_date(&event.end),
        };

        self.negated ^ bool
    }

    fn matches_string(&self, value: &str) -> bool {
        match &self.match_type {
            MatchType::Exact => value == &*self.value,
            MatchType::Contains => value.contains(&*self.value),
            MatchType::StartsWith => value.starts_with(&*self.value),
            MatchType::EndsWith => value.ends_with(&*self.value),
            MatchType::Regex => regex::Regex::new(&self.value)
                .map(|re| re.is_match(value))
                .unwrap_or(false),
            MatchType::BetweenDates => false,
            MatchType::Weekdays => false,
            MatchType::TimeOfDay => false,
        }
    }

    fn matches_date(&self, date: &Option<DateFormat>) -> bool {
        if let Some(date) = date {
            match &self.match_type {
                MatchType::BetweenDates => {
                    // Value is in the format "YYYY-MM-DD,YYYY-MM-DD"
                    let dates: Vec<&str> = self.value.split(',').collect();
                    if dates.len() == 2
                        && let (Ok(start_date), Ok(end_date)) = (
                            chrono::NaiveDate::parse_from_str(dates[0], "%Y-%m-%d"),
                            chrono::NaiveDate::parse_from_str(dates[1], "%Y-%m-%d"),
                        )
                    {
                        if let DateFormat::Date(d) = date {
                            return d >= &start_date && d <= &end_date;
                        }
                        if let DateFormat::DateTime(dt) = date {
                            let d = dt.naive_utc().date();
                            return d >= start_date && d <= end_date;
                        }
                    }
                    false
                }
                MatchType::Weekdays => {
                    // Value is in the format "Mon,Wed,Fri" (only abbreviated names)
                    let weekdays: Vec<&str> = self.value.split(',').collect();
                    let event_weekday = match date {
                        DateFormat::Date(d) => d.weekday(),
                        DateFormat::DateTime(dt) => dt.naive_utc().date().weekday(),
                    };

                    weekdays.iter().any(|wd| {
                        let wd = wd.trim();
                        // Only accept abbreviated weekday names
                        match wd {
                            "Sun" => event_weekday == chrono::Weekday::Sun,
                            "Mon" => event_weekday == chrono::Weekday::Mon,
                            "Tue" => event_weekday == chrono::Weekday::Tue,
                            "Wed" => event_weekday == chrono::Weekday::Wed,
                            "Thu" => event_weekday == chrono::Weekday::Thu,
                            "Fri" => event_weekday == chrono::Weekday::Fri,
                            "Sat" => event_weekday == chrono::Weekday::Sat,
                            _ => false,
                        }
                    })
                }
                MatchType::TimeOfDay => {
                    // Value is in the format "HH:MM,HH:MM" for time range
                    let times: Vec<&str> = self.value.split(',').collect();
                    if times.len() == 2
                        && let (Ok(start_time), Ok(end_time)) = (
                            chrono::NaiveTime::parse_from_str(times[0].trim(), "%H:%M"),
                            chrono::NaiveTime::parse_from_str(times[1].trim(), "%H:%M"),
                        )
                    {
                        let event_time = match date {
                            DateFormat::Date(_) => return false, // No time component
                            DateFormat::DateTime(dt) => dt.naive_utc().time(),
                        };
                        return event_time >= start_time && event_time <= end_time;
                    }
                    false
                }
                _ => false, // Other match types do not apply to dates
            }
        } else {
            false
        }
    }
}
