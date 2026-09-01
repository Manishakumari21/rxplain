use tasklib::models::task::{Status, Task};
use tasklib::storage::TaskStore;
use tasklib::{priority_score, validate_title};

fn main() {
    let mut store = TaskStore::new();

    let title = String::from("Write the final report");
    let task = Task::new(title);

    let id = store.insert(task);

    println!("created task {}", id);

    let score = priority_score(Status::Pending);
    println!("priority score {}", score);

    let ok = validate_title(&title);

    println!("valid title: {}", ok);
}
