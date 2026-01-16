use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::{Task, TaskStatus};

/// タスクの永続化を担当
pub struct TaskStore {
    /// .hive ディレクトリのパス
    hive_dir: PathBuf,
}

impl TaskStore {
    /// 新しいTaskStoreを作成
    pub fn new(project_root: impl Into<PathBuf>) -> Result<Self> {
        let hive_dir = project_root.into().join(".hive");

        // .hive ディレクトリがなければ作成
        if !hive_dir.exists() {
            fs::create_dir_all(&hive_dir).context("Failed to create .hive directory")?;
            fs::create_dir_all(hive_dir.join("plans")).context("Failed to create plans dir")?;
            fs::create_dir_all(hive_dir.join("worktrees")).context("Failed to create worktrees dir")?;
            fs::create_dir_all(hive_dir.join("logs")).context("Failed to create logs dir")?;
        }

        Ok(Self { hive_dir })
    }

    /// tasks.json のパスを取得
    fn tasks_file(&self) -> PathBuf {
        self.hive_dir.join("tasks.json")
    }

    /// 全タスクを読み込み
    pub fn load(&self) -> Result<Vec<Task>> {
        let path = self.tasks_file();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&path).context("Failed to read tasks.json")?;
        let tasks: Vec<Task> = serde_json::from_str(&content).context("Failed to parse tasks.json")?;
        Ok(tasks)
    }

    /// 全タスクを保存
    pub fn save(&self, tasks: &[Task]) -> Result<()> {
        let content = serde_json::to_string_pretty(tasks).context("Failed to serialize tasks")?;
        fs::write(self.tasks_file(), content).context("Failed to write tasks.json")?;
        Ok(())
    }

    /// タスクを追加
    pub fn add(&self, task: Task) -> Result<()> {
        let mut tasks = self.load()?;
        tasks.push(task);
        self.save(&tasks)
    }

    /// タスクを更新
    pub fn update(&self, task: &Task) -> Result<()> {
        let mut tasks = self.load()?;
        if let Some(existing) = tasks.iter_mut().find(|t| t.id == task.id) {
            *existing = task.clone();
            self.save(&tasks)?;
        }
        Ok(())
    }

    /// タスクを削除
    pub fn delete(&self, task_id: &str) -> Result<()> {
        let mut tasks = self.load()?;
        tasks.retain(|t| t.id != task_id);
        self.save(&tasks)
    }

    /// IDでタスクを取得
    pub fn get(&self, task_id: &str) -> Result<Option<Task>> {
        let tasks = self.load()?;
        Ok(tasks.into_iter().find(|t| t.id == task_id))
    }

    /// ステータスでフィルタして取得
    pub fn get_by_status(&self, status: TaskStatus) -> Result<Vec<Task>> {
        let tasks = self.load()?;
        Ok(tasks.into_iter().filter(|t| t.status == status).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_store_crud() {
        let dir = tempdir().unwrap();
        let store = TaskStore::new(dir.path()).unwrap();

        // Create
        let task = Task::new("Test task", "Description");
        let task_id = task.id.clone();
        store.add(task).unwrap();

        // Read
        let loaded = store.get(&task_id).unwrap().unwrap();
        assert_eq!(loaded.title, "Test task");

        // Update
        let mut updated = loaded;
        updated.title = "Updated".into();
        store.update(&updated).unwrap();

        let reloaded = store.get(&task_id).unwrap().unwrap();
        assert_eq!(reloaded.title, "Updated");

        // Delete
        store.delete(&task_id).unwrap();
        assert!(store.get(&task_id).unwrap().is_none());
    }
}
