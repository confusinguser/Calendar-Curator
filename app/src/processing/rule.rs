use crate::processing::action::Action;
use crate::processing::event::Event;
use crate::processing::matcher::Matcher;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, ToSchema, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    /// Each inner vector represents a logical AND condition,
    /// while the outer vector represents a logical OR condition.
    pub matchers: Vec<Vec<Matcher>>,
    pub actions: Vec<Action>,
}

impl Rule {
    #[allow(dead_code)]
    pub fn new(matchers: Vec<Vec<Matcher>>, actions: Vec<Action>) -> Self {
        Self {
            id: String::new(),
            matchers,
            actions,
        }
    }

    /// Applies the rule to an event.
    ///
    /// @returns A tuple containing:
    /// - An `Option<Event>` which is `None` if the event was blocked or transformed, or `Some(event)` if it was allowed.
    /// - A `bool` indicating whether the event was affected by the rule (i.e., whether it was blocked or transformed).
    pub fn apply(&self, event: Event) -> (Option<Event>, bool) {
        let mut matches = false;
        for matchers in &self.matchers {
            if matchers.iter().all(|matcher| matcher.matches(&event)) {
                matches = true; // At least one set of matchers matched
                break;
            }
        }

        if matches {
            let mut transformed_event = event.clone();
            for action in &self.actions {
                if let Some(transformed_event_after_action) =
                    action.apply(transformed_event.clone())
                {
                    transformed_event = transformed_event_after_action;
                } else {
                    // If the action returns None, it means the event is blocked
                    return (None, true);
                }
            }

            if transformed_event == event {
                // If the event was not changed by any action, it wasn't "matched"
                return (Some(transformed_event), false);
            }

            // If we reach here, the event was transformed or allowed
            (Some(transformed_event), true)
        } else {
            (Some(event), false)
        }
    }
}
