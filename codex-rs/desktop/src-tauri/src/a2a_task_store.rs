use std::collections::HashMap;
use std::collections::VecDeque;

use crate::a2a_protocol::A2aTask;
use crate::a2a_protocol::TaskState;
use crate::a2a_protocol::TaskStatus;

pub(super) const MAX_RETAINED_TASKS: usize = 128;

#[derive(Debug)]
pub(super) struct TaskStoreFull;

#[derive(Clone, Default)]
pub(super) struct TaskStore {
    tasks: HashMap<String, A2aTask>,
    insertion_order: VecDeque<String>,
}

impl TaskStore {
    pub(super) fn from_oldest_first(tasks: Vec<A2aTask>) -> Self {
        let mut store = Self::default();
        for task in tasks {
            let _ = store.insert(task);
        }
        store
    }

    pub(super) fn insert(&mut self, task: A2aTask) -> Result<Option<String>, TaskStoreFull> {
        let id = task.id.clone();
        if let Some(existing) = self.tasks.get_mut(&id) {
            *existing = task;
            return Ok(None);
        }
        if self.tasks.len() == MAX_RETAINED_TASKS {
            let Some(position) = self.insertion_order.iter().position(|id| {
                self.tasks
                    .get(id)
                    .is_some_and(|task| task.status.state != TaskState::Working)
            }) else {
                return Err(TaskStoreFull);
            };
            let Some(evicted) = self.insertion_order.remove(position) else {
                return Err(TaskStoreFull);
            };
            self.tasks.remove(&evicted);
            self.tasks.insert(id.clone(), task);
            self.insertion_order.push_back(id);
            return Ok(Some(evicted));
        }
        self.tasks.insert(id.clone(), task);
        self.insertion_order.push_back(id);
        Ok(None)
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

    pub(super) fn list_oldest_first(&self) -> Vec<A2aTask> {
        self.insertion_order
            .iter()
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

    pub(super) fn recover_working(&mut self, status: TaskStatus) -> usize {
        let mut recovered = 0;
        for task in self.tasks.values_mut() {
            if task.status.state == TaskState::Working {
                task.status = status.clone();
                recovered += 1;
            }
        }
        recovered
    }
}
