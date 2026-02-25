use crate::api::types::{CalendarStatsResponse, EventResponse};
use crate::db::{Calendar, DbState};
use crate::processing::rule::Rule;
use crate::upstream;
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use std::time::UNIX_EPOCH;
use tokio_util::io::ReaderStream;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

pub(crate) fn router() -> axum::Router<DbState> {
    let (app_router, openapi) = OpenApiRouter::new()
        .routes(routes!(create_calendar))
        .routes(routes!(get_events))
        .routes(routes!(get_calendar_url))
        .routes(routes!(update_calendar_url))
        .routes(routes!(create_rule))
        .routes(routes!(list_rules))
        .routes(routes!(update_rule))
        .routes(routes!(delete_rule))
        .routes(routes!(get_rule))
        .routes(routes!(duplicate_rule))
        .routes(routes!(reorder_rules))
        .routes(routes!(block_add))
        .routes(routes!(block_remove))
        .routes(routes!(block_list))
        .routes(routes!(allowlist_add))
        .routes(routes!(allowlist_remove))
        .routes(routes!(allowlist_list))
        .routes(routes!(get_stats))
        .split_for_parts();
    app_router
        .route("/calendars/{id}/feed", axum::routing::get(get_feed))
        .route(
            "/openapi.json",
            axum::routing::get(move || async { Json(openapi) }),
        )
}

#[derive(Deserialize, ToSchema)]
pub struct CreateCalendar {
    url: String,
}

#[derive(Serialize, ToSchema)]
pub struct CreateCalendarResponse {
    id: String,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateCalendarUrl {
    url: String,
}

#[derive(Deserialize, ToSchema)]
pub struct ReorderRulesRequest {
    rule_ids: Vec<String>,
}

#[utoipa::path(
    post,
    path = "/calendars/create",
    request_body = CreateCalendar,
    responses(
        (status = 200, description = "Created calendar", body = CreateCalendarResponse),
    )
)]
pub async fn create_calendar(
    State(db): State<DbState>,
    calendar_data: Json<CreateCalendar>,
) -> Result<Json<CreateCalendarResponse>, StatusCode> {
    let icalendar = upstream::get_icalendar(&calendar_data.url)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let calendar = Calendar::from_ical(calendar_data.url.clone(), icalendar);

    let mut db_lock = db.lock().await;
    let id = db_lock.add_calendar(calendar).await;

    let response = CreateCalendarResponse { id };
    Ok(Json(response))
}

#[utoipa::path(
    get,
    path = "/calendars/{id}/get_events",
    params(
        ("id" = String, Path, description = "The ID of the calendar")
    ),
    responses(
        (status = 200, description = "Retrieved events with transformations for the calendar", body = Vec<EventResponse>),
    )
)]
pub async fn get_events(
    State(db): State<DbState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<EventResponse>>, StatusCode> {
    let mut db_lock = db.lock().await;
    let cal = db_lock
        .get_calendar(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let blocked_events = db_lock.get_manual_blocks(&id).await.unwrap_or_default();
    let allowlisted_events = db_lock.get_manual_allowlist(&id).await.unwrap_or_default();
    drop(db_lock);

    let events: Vec<EventResponse> = cal
        .ical
        .events
        .into_iter()
        .map(|original_event| {
            let manually_blocked = blocked_events.contains(&original_event.uid);
            let manually_allowlisted = allowlisted_events.contains(&original_event.uid);
            let mut current_event = original_event.clone();
            let mut filtered_by = Vec::new();
            let mut changed_fields = Vec::new();
            let mut rule_blocked = false;

            for rule in &cal.rules {
                let (transformed_event, matched) = rule.apply(current_event.clone());
                if matched {
                    filtered_by.push(rule.id.clone());

                    if transformed_event.is_none() {
                        // If the rule blocks the event, we stop processing further rules
                        rule_blocked = true;
                    }

                    if !rule_blocked && let Some(transformed) = transformed_event {
                        // Check what fields changed
                        if original_event.summary != transformed.summary {
                            changed_fields.push("summary".to_string());
                        }
                        if original_event.description != transformed.description {
                            changed_fields.push("description".to_string());
                        }
                        if original_event.location != transformed.location {
                            changed_fields.push("location".to_string());
                        }
                        if original_event.start != transformed.start {
                            changed_fields.push("start".to_string());
                        }
                        if original_event.end != transformed.end {
                            changed_fields.push("end".to_string());
                        }

                        if !rule_blocked {
                            current_event = transformed;
                        }
                    }
                }
            }

            // Remove duplicates from changed_fields
            changed_fields.sort();
            changed_fields.dedup();

            EventResponse {
                original: original_event,
                transformed: Some(current_event),
                manually_blocked,
                rule_blocked,
                filtered_by,
                changed_fields,
                manually_allowlisted,
            }
        })
        .collect();

    Ok(Json(events))
}

#[utoipa::path(
    get,
    path = "/calendars/{id}/get_url",
    params(
        ("id" = String, Path, description = "The ID of the calendar")
    ),
    responses(
        (status = 200, description = "Retrieved url", body = String),
        (status = 404, description = "Calendar not found")
    )
)]
pub async fn get_calendar_url(
    State(db): State<DbState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db_lock = db.lock().await;
    db_lock
        .get_url_from_id(&id)
        .await
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

#[utoipa::path(
    put,
    path = "/calendars/{id}/update_url",
    request_body = UpdateCalendarUrl,
    params(
        ("id" = String, Path, description = "The ID of the calendar")
    ),
    responses(
        (status = 200, description = "Calendar URL updated successfully"),
        (status = 400, description = "Invalid URL or failed to fetch calendar"),
        (status = 404, description = "Calendar not found")
    )
)]
pub async fn update_calendar_url(
    State(db): State<DbState>,
    Path(id): Path<String>,
    body: Json<UpdateCalendarUrl>,
) -> impl IntoResponse {
    let mut db_lock = db.lock().await;
    match db_lock.update_calendar_url(&id, body.url.clone()).await {
        Ok(()) => Ok(StatusCode::OK),
        Err(err_msg) => {
            if err_msg.contains("Calendar not found") {
                Err(StatusCode::NOT_FOUND)
            } else {
                Err(StatusCode::BAD_REQUEST)
            }
        }
    }
}

pub async fn get_feed(
    State(db): State<DbState>,
    Path(id): Path<String>,
) -> Result<(HeaderMap, Body), StatusCode> {
    let mut db_lock = db.lock().await;
    let mut calendar = db_lock
        .get_calendar(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    drop(db_lock);

    calendar.last_accessed = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .as_secs(),
    );
    let ical = calendar.get_filtered_icalendar();

    let reader = std::io::Cursor::new(ical.to_string().into_bytes());
    let stream = ReaderStream::new(reader);
    let body = Body::from_stream(stream);

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "application/octet-stream".parse().unwrap(),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        "attachment; filename=\"feed.ics\"".parse().unwrap(),
    );

    Ok((headers, body))
}

#[utoipa::path(
    post,
    path = "/calendars/{id}/rules/create",
    request_body = Rule,
    params(
        ("id" = String, Path, description = "The ID of the calendar")
    ),
    responses(
        (status = 200, description = "Rule created", body = String)
    )
)]
pub async fn create_rule(
    State(db): State<DbState>,
    Path(id): Path<String>,
    body: Json<Rule>,
) -> impl IntoResponse {
    let mut db_lock = db.lock().await;
    let rule_id = db_lock.add_rule(id.clone(), body.clone().0).await;
    match rule_id {
        Some(id) => Ok(Json(id)),
        None => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[utoipa::path(
    get,
    path = "/calendars/{id}/rules/list",
    params(
        ("id" = String, Path, description = "The ID of the calendar")
    ),
    responses(
        (status = 200, description = "List of rules", body = [Rule]),
        (status = 404, description = "Calendar not found")
    )
)]
pub async fn list_rules(State(db): State<DbState>, Path(id): Path<String>) -> impl IntoResponse {
    let db_lock = db.lock().await;
    let Some(rules) = db_lock.list_rules(&id).await else {
        return Err(StatusCode::NOT_FOUND);
    };
    Ok(Json(rules))
}
#[utoipa::path(
    put,
    path = "/calendars/{id}/rules/{rule_id}/update",
    request_body = Rule,
    params(
        ("id" = String, Path, description = "The ID of the calendar"),
        ("rule_id" = String, Path, description = "The ID of the rule")
    ),
    responses(
        (status = 201, description = "Rule updated"),
        (status = 404, description = "Rule not found")
    )
)]
pub async fn update_rule(
    State(db): State<DbState>,
    Path((id, rule_id)): Path<(String, String)>,
    body: Json<Rule>,
) -> impl IntoResponse {
    let mut db_lock = db.lock().await;
    let success = db_lock.update_rule(&id, &rule_id, body.0).await;
    if !success {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(StatusCode::CREATED)
}

#[utoipa::path(
    delete,
    path = "/calendars/{id}/rules/{rule_id}/delete",
    params(
        ("id" = String, Path, description = "The ID of the calendar"),
        ("rule_id" = String, Path, description = "The ID of the rule")
    ),
    responses(
        (status = 204, description = "Rule deleted"),
    )
)]
pub async fn delete_rule(
    State(db): State<DbState>,
    Path((id, rule_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let mut db_lock = db.lock().await;
    db_lock.delete_rule(&id, &rule_id).await;
    StatusCode::NO_CONTENT
}

#[utoipa::path(
    get,
    path = "/calendars/{id}/rules/{rule_id}/get",
    params(
        ("id" = String, Path, description = "The ID of the calendar"),
        ("rule_id" = String, Path, description = "The ID of the rule")
    ),
    responses(
        (status = 200, description = "Retrieved rule", body = Rule),
        (status = 404, description = "Rule not found")
    )
)]
pub async fn get_rule(
    State(db): State<DbState>,
    Path((id, rule_id)): Path<(String, String)>,
) -> Result<Json<Rule>, StatusCode> {
    let db_lock = db.lock().await;
    let rule = db_lock
        .get_rule(&id, &rule_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(rule))
}

#[utoipa::path(
    post,
    path = "/calendars/{id}/rules/{rule_id}/duplicate",
    params(
        ("id" = String, Path, description = "The ID of the calendar"),
        ("rule_id" = String, Path, description = "The ID of the rule to duplicate")
    ),
    responses(
        (status = 200, description = "Rule duplicated successfully", body = String),
        (status = 404, description = "Rule or calendar not found")
    )
)]
pub async fn duplicate_rule(
    State(db): State<DbState>,
    Path((id, rule_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let mut db_lock = db.lock().await;
    let new_rule_id = db_lock.duplicate_rule(&id, &rule_id).await;
    match new_rule_id {
        Some(id) => Ok(Json(id)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

#[utoipa::path(
    put,
    path = "/calendars/{id}/rules/reorder",
    request_body = ReorderRulesRequest,
    params(
        ("id" = String, Path, description = "The ID of the calendar")
    ),
    responses(
        (status = 200, description = "Rules reordered successfully"),
        (status = 400, description = "Invalid rule order or missing rules"),
        (status = 404, description = "Calendar not found")
    )
)]
pub async fn reorder_rules(
    State(db): State<DbState>,
    Path(id): Path<String>,
    body: Json<ReorderRulesRequest>,
) -> impl IntoResponse {
    let mut db_lock = db.lock().await;
    let success = db_lock.reorder_rules(&id, body.rule_ids.clone()).await;
    if success {
        Ok(StatusCode::OK)
    } else {
        Err(StatusCode::BAD_REQUEST)
    }
}

#[utoipa::path(
    post,
    path = "/calendars/{id}/block/add",
    request_body = String,
    params(
        ("id" = String, Path, description = "The ID of the calendar")
    ),
    responses(
        (status = 200, description = "Rule created", body = String)
    )
)]
pub async fn block_add(
    State(db): State<DbState>,
    Path(id): Path<String>,
    body: Json<String>,
) -> impl IntoResponse {
    let mut db_lock = db.lock().await;
    db_lock.add_manual_block(id.clone(), body.0).await;
    StatusCode::NO_CONTENT
}

#[utoipa::path(
    post,
    path = "/calendars/{id}/block/remove",
    request_body = String,
    params(
        ("id" = String, Path, description = "The ID of the calendar")
    ),
    responses(
        (status = 200, description = "Rule created", body = String)
    )
)]
pub async fn block_remove(
    State(db): State<DbState>,
    Path(id): Path<String>,
    body: Json<String>,
) -> impl IntoResponse {
    let mut db_lock = db.lock().await;
    let success = db_lock.remove_manual_block(&id, &body.0).await;
    if !success {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

#[utoipa::path(
    get,
    path = "/calendars/{id}/block/list",
    request_body = String,
    params(
        ("id" = String, Path, description = "The ID of the calendar")
    ),
    responses(
        (status = 200, description = "List of all blocks", body = HashSet<String>)
    )
)]
pub async fn block_list(State(db): State<DbState>, Path(id): Path<String>) -> impl IntoResponse {
    let db_lock = db.lock().await;
    let Some(set) = db_lock.get_manual_blocks(&id).await else {
        return Err(StatusCode::NOT_FOUND);
    };

    Ok(Json(set))
}

#[utoipa::path(
    post,
    path = "/calendars/{id}/allowlist/add",
    request_body = String,
    params(
        ("id" = String, Path, description = "The ID of the calendar")
    ),
    responses(
        (status = 204, description = "Event added to allowlist")
    )
)]
pub async fn allowlist_add(
    State(db): State<DbState>,
    Path(id): Path<String>,
    body: Json<String>,
) -> impl IntoResponse {
    let mut db_lock = db.lock().await;
    db_lock.add_manual_allowlist(id.clone(), body.0).await;
    StatusCode::NO_CONTENT
}

#[utoipa::path(
    post,
    path = "/calendars/{id}/allowlist/remove",
    request_body = String,
    params(
        ("id" = String, Path, description = "The ID of the calendar")
    ),
    responses(
        (status = 204, description = "Event removed from allowlist"),
        (status = 404, description = "Event not found in allowlist")
    )
)]
pub async fn allowlist_remove(
    State(db): State<DbState>,
    Path(id): Path<String>,
    body: Json<String>,
) -> impl IntoResponse {
    let mut db_lock = db.lock().await;
    let success = db_lock.remove_manual_allowlist(&id, &body.0).await;
    if !success {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

#[utoipa::path(
    get,
    path = "/calendars/{id}/allowlist/list",
    params(
        ("id" = String, Path, description = "The ID of the calendar")
    ),
    responses(
        (status = 200, description = "List of all allowlisted events", body = HashSet<String>)
    )
)]
pub async fn allowlist_list(
    State(db): State<DbState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let db_lock = db.lock().await;
    let Some(set) = db_lock.get_manual_allowlist(&id).await else {
        return Err(StatusCode::NOT_FOUND);
    };

    Ok(Json(set))
}

#[utoipa::path(
    get,
    path = "/stats",
    responses(
        (status = 200, description = "Retrieved calendar statistics", body = CalendarStatsResponse),
    )
)]
pub async fn get_stats(
    State(db): State<DbState>,
) -> Result<Json<CalendarStatsResponse>, StatusCode> {
    let db_lock = db.lock().await;
    let mut active_calendars = 0;
    let calendars = db_lock.list_calendars().await;

    for calendar in calendars {
        let Some(last_accessed) = calendar.last_accessed else {
            continue;
        };
        let Ok(duration_since_last_access) = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH + std::time::Duration::from_secs(last_accessed))
            .map_err(|err| {
                eprintln!("Error calculating duration since last access: {}", err);
            })
        else {
            continue;
        };

        if duration_since_last_access < std::time::Duration::from_secs(2 * 24 * 60 * 60) {
            active_calendars += 1;
        }
    }

    let stats = CalendarStatsResponse { active_calendars };

    Ok(Json(stats))
}
