use crate::error::SyntaxError;
use crate::utils::{DateFormat, parse_datetime};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, ToSchema, Serialize, Deserialize, Clone, PartialEq)]
pub struct Event {
    pub(crate) start: Option<DateFormat>,
    pub(crate) end: Option<DateFormat>,
    pub(crate) uid: String,
    pub(crate) timestamp: String,
    pub(crate) last_modified: String,
    pub(crate) summary: String,
    pub(crate) location: String,
    pub(crate) description: String,
}

impl Event {
    pub(crate) fn from_string(event_str: &str) -> Result<Event, SyntaxError> {
        let mut event = Event {
            start: None,
            end: None,
            uid: String::new(),
            timestamp: String::new(),
            last_modified: String::new(),
            summary: String::new(),
            location: String::new(),
            description: String::new(),
        };

        for (i, line) in event_str.lines().enumerate() {
            if line.starts_with("DTSTART") {
                event.start = Some(
                    parse_datetime(line.replace("DTSTART", "").trim())
                        .map_err(|e| e.with_line(i + 1))?,
                );
            } else if line.starts_with("DTEND") {
                event.end = Some(
                    parse_datetime(line.replace("DTEND", "").trim())
                        .map_err(|e| e.with_line(i + 1))?,
                );
            } else if line.starts_with("UID:") {
                event.uid = line.replace("UID:", "").trim().to_string();
            } else if line.starts_with("DTSTAMP:") {
                event.timestamp = line.replace("DTSTAMP:", "").trim().to_string();
            } else if line.starts_with("LAST-MODIFIED:") {
                event.last_modified = line.replace("LAST-MODIFIED:", "").trim().to_string();
            } else if line.starts_with("SUMMARY:") {
                event.summary = line.replace("SUMMARY:", "").trim().to_string();
            } else if line.starts_with("LOCATION:") {
                event.location = line.replace("LOCATION:", "").trim().to_string();
            } else if line.starts_with("DESCRIPTION:") {
                event.description = line
                    .replace("DESCRIPTION:", "")
                    .replace("\\n", "\n")
                    .trim()
                    .to_string();
            }
        }
        Ok(event)
    }
}
