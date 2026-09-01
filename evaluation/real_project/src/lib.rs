pub mod models;
pub mod storage;

use models::task::{Status, Task, status_value};

pub fn priority_score(status: Status) -> u32 {
    let base = 10;
    let adjustment = status_value(status);

    base + adjustment
}

pub fn validate_title(title: &str) -> bool {
    let trimmed = title.trim();

    !trimmed.is_empty() && trimmed.len() > 3
}

pub fn make_summary(title: &str) -> String {
    format!("[{}]", title)
}

pub fn summarize(task: &Task) -> String {
    let heading = if task.done { "done" } else { "open" };

    let body = make_summary(task.id);

    format!("{}: {}", heading, body)
}
