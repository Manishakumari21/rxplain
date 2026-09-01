use std::collections::HashMap;

use crate::models::task::Task;

pub struct TaskStore {
    tasks: HashMap<u32, Task>,
    next_id: u32,
}

impl TaskStore {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn insert(&mut self, task: Task) -> u32 {
        self.tasks.insert(self.next_id, task);
        self.next_id += 1;
        self.next_id - 1
    }

    pub fn get(&self, id: u32) -> Option<&Task> {
        self.tasks.get(&id)
    }

    pub fn set_done(&mut self, id: u32) {
        if let Some(task) = self.tasks.get_mut(&id) {
            task.done = true;
        }
    }

    pub fn toggle(&mut self, id: u32) {
        if let Some(task) = self.tasks.get(&id) {
            let current = task.done;
            task.done = !current;
        }
    }

    pub fn count_pending(&self) -> usize {
        self.tasks.values().filter(|task| !task.done).count()
    }
}
