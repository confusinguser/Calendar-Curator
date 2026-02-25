use crate::error::SyntaxError;
use crate::processing::event::Event;
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};
use utoipa::ToSchema;

/// This is a representation of an iCalendar object from an iCal file. There is also [Calendar] which is the calendar object used internally when processing
#[derive(Debug, Clone, ToSchema, Serialize, Deserialize, Default)]
pub struct ICalendar {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) method: String,
    pub(crate) version: String,
    pub(crate) prodid: String,
    pub(crate) calscale: String,
    pub(crate) published_ttl: String,
    pub events: Vec<Event>,
}

impl fmt::Display for ICalendar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "BEGIN:VCALENDAR\n")?;
        writeln!(f, "X-WR-CALNAME:{}\n", self.name)?;
        writeln!(f, "X-WR-CALDESC:{}\n", self.description)?;
        writeln!(f, "METHOD:{}\n", self.method)?;
        writeln!(f, "VERSION:{}\n", self.version)?;
        writeln!(f, "PRODID:{}\n", self.prodid)?;
        writeln!(f, "CALSCALE:{}\n", self.calscale)?;
        writeln!(f, "X-PUBLISHED-TTL:{}\n", self.published_ttl)?;
        writeln!(f, "NAME:{}\n", self.name)?;

        for event in &self.events {
            writeln!(f, "BEGIN:VEVENT\n")?;
            if let Some(start) = &event.start {
                writeln!(f, "DTSTART{}\n", start.to_ical_str())?;
            }
            if let Some(end) = &event.end {
                writeln!(f, "DTEND{}\n", end.to_ical_str())?;
            }
            writeln!(f, "UID:{}\n", event.uid)?;
            writeln!(f, "DTSTAMP:{}\n", event.timestamp)?;
            writeln!(f, "LAST-MODIFIED:{}\n", event.last_modified)?;
            writeln!(f, "SUMMARY:{}\n", event.summary)?;
            writeln!(f, "LOCATION:{}\n", event.location)?;
            writeln!(
                f,
                "DESCRIPTION:{}\n",
                event.description.replace("\n", "\\n")
            )?;
            writeln!(f, "END:VEVENT\n")?;
        }

        writeln!(f, "END:VCALENDAR\n")
    }
}
impl ICalendar {
    pub fn from_string(calendar_str: &str) -> Result<ICalendar, Box<dyn Error>> {
        let mut calendar = ICalendar::default();

        let mut event_str = String::new();
        let mut in_event = false;

        // Iterate through each line of the calendar string and put folded lines together
        // to handle multi-line properties correctly.
        let mut unfolded_lines = String::new();
        for line in calendar_str.lines() {
            if line.starts_with(" ") || line.starts_with("\t") {
                // This line is a continuation of the previous line
                unfolded_lines.push_str(line.trim_start());
            } else {
                unfolded_lines.push('\n'); // Add a newline before the new line
                unfolded_lines.push_str(line);
            }
        }
        let calendar_str = unfolded_lines.replace("\\,", ",").replace("\\;", ";");

        for (i, line) in calendar_str.lines().enumerate() {
            if in_event {
                if line.starts_with("BEGIN:VEVENT") {
                    // If we encounter another BEGIN:VEVENT while already in an event, we should not process it
                    return Err(SyntaxError::new(
                        "Nested BEGIN:VEVENT found".to_string(),
                        Some(i + 1),
                    )
                    .into());
                }

                if line.starts_with("END:VEVENT") {
                    // End of the current event, process it
                    calendar
                        .events
                        .push(Event::from_string(&event_str).map_err(|e| {
                            if let Some(line_num) = e.line {
                                dbg!(line_num, i);
                                e.with_line(line_num + i) // Adjust line number based on current index
                            } else {
                                e
                            }
                        })?);
                    in_event = false; // Reset in_event flag
                    continue;
                }

                event_str.push_str(line);
                event_str.push('\n');
            }

            if line.starts_with("BEGIN:VCALENDAR") {
                continue;
            } else if line.starts_with("END:VCALENDAR") {
                break;
            } else if line.starts_with("X-WR-CALNAME:") {
                calendar.name = line.replace("X-WR-CALNAME:", "").trim().to_string();
            } else if line.starts_with("X-WR-CALDESC:") {
                calendar.description = line
                    .replace("X-WR-CALDESC:", "")
                    .trim()
                    .to_string()
                    .replace("\\n", "\n");
            } else if line.starts_with("METHOD:") {
                calendar.method = line.replace("METHOD:", "").trim().to_string();
            } else if line.starts_with("VERSION:") {
                calendar.version = line.replace("VERSION:", "").trim().to_string();
            } else if line.starts_with("PRODID:") {
                calendar.prodid = line.replace("PRODID:", "").trim().to_string();
            } else if line.starts_with("CALSCALE:") {
                calendar.calscale = line.replace("CALSCALE:", "").trim().to_string();
            } else if line.starts_with("X-PUBLISHED-TTL:") {
                calendar.published_ttl = line.replace("X-PUBLISHED-TTL:", "").trim().to_string();
            } else if line.starts_with("BEGIN:VEVENT") {
                event_str.clear(); // Start a new event
                in_event = true;
            }
        }
        Ok(calendar)
    }
}
