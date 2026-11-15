use crate::processing::event::Event;
use crate::utils::DateFormat;
use chrono::{DateTime, Duration, Local, NaiveDateTime, NaiveTime, Utc};
use rand::prelude::*;
use uuid::Uuid;

#[allow(dead_code)]
const EVENT_TITLES: [&str; 12] = [
    "Team Meeting",
    "Project Review",
    "Lunch with Client",
    "Code Review",
    "Sprint Planning",
    "Tech Talk",
    "Product Demo",
    "Brainstorming Session",
    "One-on-One Meeting",
    "Conference Call",
    "Training Workshop",
    "Quarterly Planning",
];

#[allow(dead_code)]
const EVENT_LOCATIONS: [&str; 8] = [
    "Conference Room A",
    "Main Office",
    "Coffee Shop",
    "Zoom Meeting",
    "Client Office",
    "Meeting Room 3",
    "Virtual Call",
    "Webinar",
];

#[allow(dead_code)]
const EVENT_DESCRIPTIONS: [&str; 10] = [
    "Discuss project progress and upcoming milestones",
    "Review recent code changes and discuss implementation details",
    "Plan for the next sprint and assign tasks to team members",
    "Catch up on recent developments and plan next steps",
    "Demonstrate new features to the product team",
    "Learn about new technologies and best practices",
    "Generate new ideas for product improvements",
    "Evaluate performance and discuss career growth",
    "Connect with remote team members and share updates",
    "Deep dive into technical concepts and implementation details",
];

/// Generate random events around the current date
#[allow(dead_code)]
pub fn generate_example_events(count: usize) -> Vec<Event> {
    let mut rng = rand::rng();
    let today = Local::now().naive_local().date();

    let mut events = Vec::with_capacity(count);

    for _i in 0..count {
        // Generate a date within +/- 30 days from today
        let days_offset = rng.random_range(-30..=30);
        let event_date = today + Duration::days(days_offset);

        // Generate start time (between 8 AM and 5 PM)
        let hour = rng.random_range(8..=17);
        let minute = [0, 15, 30, 45][rng.random_range(0..4)]; // 15-minute increments
        let start_time = NaiveTime::from_hms_opt(hour, minute, 0).unwrap();
        let start_datetime =
            DateTime::from_naive_utc_and_offset(NaiveDateTime::new(event_date, start_time), Utc);

        // Generate duration (30 mins, 1 hour, 1.5 hours, or 2 hours)
        let duration_mins = [30, 60, 90, 120][rng.random_range(0..4)];
        let end_datetime = start_datetime + Duration::minutes(duration_mins);

        // Select random title, location, and description
        let title = EVENT_TITLES[rng.random_range(0..EVENT_TITLES.len())];
        let location = EVENT_LOCATIONS[rng.random_range(0..EVENT_LOCATIONS.len())];
        let description = EVENT_DESCRIPTIONS[rng.random_range(0..EVENT_DESCRIPTIONS.len())];

        // Create event
        let event = Event {
            uid: format!("example-event-{}", Uuid::new_v4()),
            timestamp: start_datetime.to_string(),
            last_modified: start_datetime.to_string(),
            summary: title.to_string(),
            description: description.to_string(),
            location: location.to_string(),
            start: Some(DateFormat::DateTime(start_datetime)),
            end: Some(DateFormat::DateTime(end_datetime)),
        };

        events.push(event);
    }

    events
}

