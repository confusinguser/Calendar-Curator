pub(crate) use crate::processing::calendar::Calendar;
use crate::processing::event::Event;
use crate::processing::ical::ICalendar;
use crate::processing::rule::Rule;
use crate::upstream;
use std::collections::{HashMap, HashSet};
use std::io::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use uuid::Uuid;

// Cache structure to store events with timestamps
#[derive(Clone, Debug)]
struct EventCache {
    events: Vec<Event>,
    cached_at: u64,
}

impl EventCache {
    fn new(events: Vec<Event>) -> Self {
        let cached_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self { events, cached_at }
    }

    fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now - self.cached_at > 15 * 60 // 15 minutes
    }
}

static EVENT_CACHE: LazyLock<Mutex<HashMap<String, EventCache>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static TASKS: LazyLock<Mutex<JoinSet<Result<(), Error>>>> =
    LazyLock::new(|| Mutex::new(JoinSet::new()));

#[derive(Clone, Debug)]
pub struct Db {
    path: PathBuf,
    calendars: HashMap<String, Calendar>,
}

impl Db {
    pub async fn new(path: String) -> Self {
        let path = PathBuf::from(path);
        let calendars = match fs::read_to_string(&path).await {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => HashMap::new(),
        };
        Db { path, calendars }
    }

    pub async fn save_data_bg(&self) {
        let self_clone = self.clone();
        TASKS.lock().await.spawn(async move {
            match File::create(&self_clone.path).await {
                Ok(mut file) => {
                    let data = serde_json::to_string(&self_clone.calendars)?;
                    file.write_all(data.as_bytes()).await?;
                    file.flush().await?;
                }
                Err(e) => {
                    eprintln!("Failed to create file {}: {}", self_clone.path.display(), e);
                    return Err(e);
                }
            };
            Ok(())
        });
    }

    pub async fn list_calendars(&self) -> Vec<Calendar> {
        self.calendars.values().cloned().collect()
    }

    pub async fn add_calendar(&mut self, mut calendar: Calendar) -> String {
        let id = calendar.id.clone();
        // We don't want to store all events in the DB, just the calendar metadata
        calendar.ical.events = ICalendar::default().events;
        self.calendars.insert(id.clone(), calendar);
        self.save_data_bg().await; // Auto-save
        id
    }

    pub async fn get_calendar(&mut self, id: &str) -> Option<Calendar> {
        let calendars = self.calendars.get(id);
        let mut calendar = calendars.cloned()?;
        // Update last accessed time
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some(calendar_ref) = self.calendars.get_mut(id) {
            calendar_ref.last_accessed = Some(now);
            self.save_data_bg().await;
        }

        // Check if we have cached events that are not expired
        let mut event_cache = EVENT_CACHE.lock().await;
        let cache = event_cache.get_mut(id);
        if let Some(cached) = cache
            && !cached.is_expired()
        {
            calendar.ical.events = cached.events.clone();
            return Some(calendar);
        }

        // Cache is expired or doesn't exist, fetch fresh events
        if let Ok(fresh_ical) = upstream::get_icalendar(&calendar.url).await {
            let events = fresh_ical.events.clone();
            calendar.ical = fresh_ical;

            // Update cache
            event_cache.insert(id.to_string(), EventCache::new(events));
        }
        Some(calendar)
    }

    // Rule management

    pub async fn add_rule(&mut self, calendar_id: String, mut rule: Rule) -> Option<String> {
        rule.id = Uuid::new_v4().to_string();
        let calendar = self.calendars.get_mut(&calendar_id)?;

        let id = rule.id.clone();
        calendar.add_rule(rule);
        self.save_data_bg().await;

        Some(id)
    }

    pub async fn list_rules(&self, calendar_id: &str) -> Option<Vec<Rule>> {
        self.calendars.get(calendar_id).map(|cal| cal.rules.clone())
    }

    pub async fn get_rule(&self, calendar_id: &str, rule_id: &str) -> Option<Rule> {
        self.calendars.get(calendar_id).and_then(|calendar| {
            calendar
                .rules
                .iter()
                .find(|rule| rule.id == rule_id)
                .cloned()
        })
    }

    pub async fn update_rule(&mut self, calendar_id: &str, rule_id: &str, rule: Rule) -> bool {
        if let Some(calendar) = self.calendars.get_mut(calendar_id)
            && let Some(existing_rule) = calendar.rules.iter_mut().find(|rule| rule.id == rule_id)
        {
            *existing_rule = rule;
            self.save_data_bg().await;
            return true;
        }
        false
    }

    pub async fn delete_rule(&mut self, calendar_id: &str, rule_id: &str) {
        if let Some(calendar) = self.calendars.get_mut(calendar_id) {
            calendar.rules.retain(|rule| rule.id != rule_id);
            self.save_data_bg().await;
        }
    }

    pub async fn duplicate_rule(&mut self, calendar_id: &str, rule_id: &str) -> Option<String> {
        if let Some(calendar) = self.calendars.get_mut(calendar_id) {
            // Find the original rule and its position
            let rule_position = calendar.rules.iter().position(|rule| rule.id == rule_id)?;
            let original_rule = calendar.rules[rule_position].clone();

            // Create a new rule with a new ID but same content
            let mut duplicated_rule = original_rule;
            duplicated_rule.id = Uuid::new_v4().to_string();
            let new_rule_id = duplicated_rule.id.clone();

            // Insert the duplicated rule right after the original rule
            calendar.rules.insert(rule_position + 1, duplicated_rule);

            self.save_data_bg().await;
            return Some(new_rule_id);
        }
        None
    }

    pub async fn reorder_rules(&mut self, calendar_id: &str, rule_ids: Vec<String>) -> bool {
        if let Some(calendar) = self.calendars.get_mut(calendar_id) {
            // Create a map of rule_id to rule for quick lookup
            let rule_map: HashMap<String, Rule> = calendar
                .rules
                .iter()
                .map(|rule| (rule.id.clone(), rule.clone()))
                .collect();

            // Rebuild the rules vector in the new order
            let mut new_rules = Vec::new();
            for rule_id in rule_ids {
                if let Some(rule) = rule_map.get(&rule_id) {
                    new_rules.push(rule.clone());
                }
            }

            // Only update if we have the same number of rules (no rules lost)
            if new_rules.len() == calendar.rules.len() {
                calendar.rules = new_rules;
                self.save_data_bg().await;
                return true;
            }
        }
        false
    }

    pub async fn add_manual_block(&mut self, calendar_id: String, event_uid: String) {
        if let Some(calendar) = self.calendars.get_mut(&calendar_id) {
            calendar.manually_allowlisted.remove(&event_uid);
            calendar.manually_blocked.insert(event_uid);
            self.save_data_bg().await;
        }
    }

    pub async fn get_manual_blocks(&self, calendar_id: &str) -> Option<HashSet<String>> {
        self.calendars
            .get(calendar_id)
            .map(|cal| cal.manually_blocked.clone())
    }

    pub async fn remove_manual_block(&mut self, calendar_id: &str, block: &str) -> bool {
        if let Some(calendar) = self.calendars.get_mut(calendar_id) {
            let removed = calendar.manually_blocked.remove(block);
            if removed {
                self.save_data_bg().await;
            }
            return removed;
        }
        false
    }

    pub async fn add_manual_allowlist(&mut self, calendar_id: String, event_uid: String) {
        if let Some(calendar) = self.calendars.get_mut(&calendar_id) {
            calendar.manually_blocked.remove(&event_uid);
            calendar.manually_allowlisted.insert(event_uid);
            self.save_data_bg().await;
        }
    }

    pub async fn get_manual_allowlist(&self, calendar_id: &str) -> Option<HashSet<String>> {
        self.calendars
            .get(calendar_id)
            .map(|cal| cal.manually_allowlisted.clone())
    }

    pub async fn remove_manual_allowlist(&mut self, calendar_id: &str, event_uid: &str) -> bool {
        if let Some(calendar) = self.calendars.get_mut(calendar_id) {
            let removed = calendar.manually_allowlisted.remove(event_uid);
            if removed {
                self.save_data_bg().await;
            }
            return removed;
        }
        false
    }

    pub async fn get_url_from_id(&self, calendar_id: &str) -> Option<String> {
        self.calendars.get(calendar_id).map(|cal| cal.url.clone())
    }

    pub async fn update_calendar_url(
        &mut self,
        calendar_id: &str,
        new_url: String,
    ) -> Result<(), String> {
        // Check if calendar exists
        let Some(mut calendar) = self.calendars.get(calendar_id).cloned() else {
            return Err("Calendar not found".to_string());
        };

        // Update the calendar with new URL and fresh data
        calendar.url = new_url;
        calendar.ical = ICalendar::default(); // We don't store events in DB, they're cached separately

        // Update in database
        self.calendars.insert(calendar_id.to_string(), calendar);

        // Clear the cache for this calendar so fresh events will be fetched next time
        let mut event_cache = EVENT_CACHE.lock().await;
        event_cache.remove(calendar_id);

        self.save_data_bg().await; // Auto-save
        Ok(())
    }

    pub async fn cleanup_old_calendars(&mut self) -> Vec<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // One week in seconds = 7 days * 24 hours * 60 minutes * 60 seconds
        let one_week_in_seconds = 7 * 24 * 60 * 60;
        let cutoff_time = now - one_week_in_seconds;

        let mut removed_ids = Vec::new();

        // Identify calendars to remove (older than one week)
        let to_remove: Vec<String> = self
            .calendars
            .iter()
            .filter(|(_, calendar)| {
                match calendar.last_accessed {
                    Some(last_accessed) => last_accessed < cutoff_time,
                    None => true, // Remove calendars with no access timestamp
                }
            })
            .map(|(id, _)| id.clone())
            .collect();

        // Remove identified calendars
        for id in &to_remove {
            self.calendars.remove(id);

            // Also clean up the event cache for this calendar
            let mut event_cache = EVENT_CACHE.lock().await;
            event_cache.remove(id);

            removed_ids.push(id.clone());
        }

        if !removed_ids.is_empty() {
            self.save_data_bg().await;
        }

        removed_ids
    }
}

// Database instance type
pub type DbState = Arc<Mutex<Db>>;

// Create database instance
pub async fn create_db_instance(path: String) -> DbState {
    Arc::new(Mutex::new(Db::new(path).await))
}
