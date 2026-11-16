use serde_json::Value;
use workyard_core::Store;

fn store() -> Result<Store, String> {
    Store::default_path()
        .map(Store::new)
        .map_err(|e| e.to_string())
}
fn wrap(value: workyard_core::Result<Value>) -> Result<Value, String> {
    value.map_err(|e| serde_json::to_string(&e).unwrap_or_else(|_| e.to_string()))
}

#[tauri::command]
fn list_projects() -> Result<Value, String> {
    wrap(store()?.list_projects())
}
#[tauri::command]
fn view_project(project: String) -> Result<Value, String> {
    wrap(store()?.view_project(&project))
}
#[tauri::command]
fn list_boards() -> Result<Value, String> {
    wrap(store()?.list_boards())
}
#[tauri::command]
fn view_board(board: String) -> Result<Value, String> {
    wrap(store()?.view_board(&board))
}
#[tauri::command]
fn view_card(card: String) -> Result<Value, String> {
    wrap(store()?.view_card(&card))
}
#[tauri::command]
fn add_card_note(card: String, body: String) -> Result<Value, String> {
    wrap(store()?.add_card_note("ui", &card, &body))
}
#[tauri::command]
fn latest_event_id() -> Result<i64, String> {
    store()?.latest_event_id().map_err(|e| e.to_string())
}
#[tauri::command]
fn poll_events(since: i64, limit: i64) -> Result<Value, String> {
    wrap(store()?.poll_events(since, limit))
}
#[tauri::command]
fn list_questions() -> Result<Value, String> {
    wrap(store()?.list_questions(None, None))
}
#[tauri::command]
fn answer_question(
    question: String,
    answer: String,
    note: String,
    expected_version: i64,
) -> Result<Value, String> {
    wrap(store()?.resolve_question(
        "ui",
        &question,
        &answer,
        Some(&note),
        expected_version,
        false,
    ))
}
#[tauri::command]
fn assign_question_stakeholder(question: String, stakeholder: String, expected_version: i64) -> Result<Value, String> {
    wrap(store()?.assign_question_stakeholder("ui", &question, &stakeholder, expected_version))
}
#[tauri::command]
fn unassign_question_stakeholder(question: String, expected_version: i64) -> Result<Value, String> {
    wrap(store()?.unassign_question_stakeholder("ui", &question, expected_version))
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_projects,
            view_project,
            list_boards,
            view_board,
            view_card,
            add_card_note,
            latest_event_id,
            poll_events,
            list_questions,
            answer_question,
            assign_question_stakeholder,
            unassign_question_stakeholder
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Workyard");
}
