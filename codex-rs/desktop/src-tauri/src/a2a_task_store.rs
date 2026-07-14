use std::collections::HashMap;
use std::collections::VecDeque;

use crate::a2a_protocol::A2aTask;
use crate::a2a_protocol::TaskState;
use crate::a2a_protocol::TaskStatus;

pub(super) const MAX_RETAINED_TASKS: usize = 128;

#[derive(Default)]
pub(super) struct TaskStore {
    tasks: HashMap<String, A2aTask>,
    insertion_order: VecDeque<String>,
}

impl TaskStore {
    pub(super) fn insert(&mut self, task: A2aTask) -> Option<String> {
        let id = task.id.clone();
        if self.tasks.insert(id.clone(), task).is_none() {
            self.insertion_order.push_back(id);
        }
        if self.tasks.len() <= MAX_RETAINED_TASKS {
            return None;
        }
        let evicted = self.insertion_order.pop_front()?;
        self.tasks.remove(&evicted);
        Some(evicted)
    }

    pub(super) fn get(&self, id: &str) -> Option<A2aTask> {
        self.tasks.get(id).cloned()
    }

    pub(super) fn get_mut(&mut self, id: &str) -> Option<&mut A2aTask> {
        self.tasks.get_mut(id)
    }

    pub(super) fn list_newest_first(&self) -> Vec<A2aTask> {
        self.insertion_order
            .iter()
            .rev()
            .filter_map(|id| self.tasks.get(id).cloned())
            .collect()
    }

    pub(super) fn cancel(&mut self, id: &str, status: TaskStatus) -> Option<A2aTask> {
        let task = self.tasks.get_mut(id)?;
        if task.status.state == TaskState::Working {
            task.status = status;
        }
        Some(task.clone())
    }

    pub(super) fn working_ids(&self) -> Vec<String> {
        self.tasks
            .values()
            .filter(|task| task.status.state == TaskState::Working)
            .map(|task| task.id.clone())
            .collect()
    }
}
