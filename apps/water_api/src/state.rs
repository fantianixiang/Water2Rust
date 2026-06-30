//! 任务注册表（内存态），对应 workshop 的 `task_manager`。
//!
//! 当前为骨架：任务在后台 tokio 任务中执行，状态存于内存 `Mutex<HashMap>`。
//! 真实业务执行接入 `water-hydro` / `water-fclass` / `water-edge-depth`。

use std::collections::HashMap;
use std::sync::Mutex;

use uuid::Uuid;

use crate::models::{TaskSnapshot, TaskState};

/// 单个任务的内部状态。
#[derive(Debug, Clone)]
pub struct TaskRecord {
    pub state: TaskState,
    pub progress: u8,
    pub logs: Vec<String>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

impl TaskRecord {
    fn new() -> Self {
        Self {
            state: TaskState::Pending,
            progress: 0,
            logs: Vec::new(),
            result: None,
            error: None,
        }
    }
}

/// 应用共享状态。
pub struct AppState {
    tasks: Mutex<HashMap<String, TaskRecord>>,
}

impl AppState {
    pub fn new() -> Self {
        Self { tasks: Mutex::new(HashMap::new()) }
    }

    /// 创建任务并返回 task_id。
    pub fn create_task(&self) -> String {
        let id = Uuid::new_v4().to_string();
        self.tasks.lock().unwrap().insert(id.clone(), TaskRecord::new());
        id
    }

    /// 更新任务状态。
    pub fn update<F: FnOnce(&mut TaskRecord)>(&self, task_id: &str, f: F) {
        if let Some(rec) = self.tasks.lock().unwrap().get_mut(task_id) {
            f(rec);
        }
    }

    /// 获取任务快照。
    pub fn snapshot(&self, task_id: &str) -> Option<TaskSnapshot> {
        let guard = self.tasks.lock().unwrap();
        let rec = guard.get(task_id)?;
        Some(TaskSnapshot {
            task_id: task_id.to_string(),
            state: rec.state,
            progress: rec.progress,
            logs: rec.logs.clone(),
            result: rec.result.clone(),
            error: rec.error.clone(),
        })
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
