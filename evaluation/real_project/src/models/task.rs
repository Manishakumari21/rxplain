pub struct Task {
    pub id: u32,
    pub title: String,
    pub done: bool,
}

pub enum Status {
    Pending,
    InProgress,
    Done,
}

impl Task {
    pub fn new(title: String) -> Self {
        Self {
            id: 0,
            title,
            done: false,
        }
    }
}

pub fn status_label(status: Status) -> &'static str {
    match status {
        Status::Pending => "pending",
        Status::InProgress => "in progress",
        Status::Done => "done",
    }
}

pub fn status_value(status: Status) -> u32 {
    match status {
        Status::Pending => 0,
        Status::InProgress => 1,
        Status::Done => 2,
    }
}

pub fn ordered_labels(statuses: &[Status]) -> Vec<String> {
    let mut sorted = statuses.to_vec();

    sorted.sort();

    sorted.iter().map(|status| status_label(*status).to_string()).collect()
}
