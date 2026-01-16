use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// タスクのステータス
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Todo,
    InProgress,
    Review,
    Done,
    Cancelled,
}

impl TaskStatus {
    /// ステータスの表示名を取得
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Todo => "Todo",
            Self::InProgress => "In Progress",
            Self::Review => "Review",
            Self::Done => "Done",
            Self::Cancelled => "Cancelled",
        }
    }

    /// カラムインデックスに変換 (Cancelledは表示しない)
    pub fn to_column_index(&self) -> Option<usize> {
        match self {
            Self::Todo => Some(0),
            Self::InProgress => Some(1),
            Self::Review => Some(2),
            Self::Done => Some(3),
            Self::Cancelled => None,
        }
    }

    /// カラムインデックスからステータスに変換
    pub fn from_column_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Todo),
            1 => Some(Self::InProgress),
            2 => Some(Self::Review),
            3 => Some(Self::Done),
            _ => None,
        }
    }
}

/// タスク
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// 一意なID
    pub id: String,
    /// タスクのタイトル
    pub title: String,
    /// タスクの説明
    pub description: String,
    /// 現在のステータス
    pub status: TaskStatus,
    /// 割り当てられたエージェント名 (claude, gemini, codex)
    pub agent: Option<String>,
    /// 作業ブランチ名
    pub branch: Option<String>,
    /// worktreeのパス
    pub worktree: Option<String>,
    /// 作成日時
    pub created_at: DateTime<Utc>,
    /// 開始日時
    pub started_at: Option<DateTime<Utc>>,
    /// 完了日時
    pub completed_at: Option<DateTime<Utc>>,
    /// ログファイルのパス
    pub output_log: Option<String>,
}

impl Task {
    /// 新しいタスクを作成
    pub fn new(title: impl Into<String>, description: impl Into<String>) -> Self {
        let id = format!("task-{}", Uuid::new_v4().to_string().split('-').next().unwrap());
        Self {
            id,
            title: title.into(),
            description: description.into(),
            status: TaskStatus::Todo,
            agent: None,
            branch: None,
            worktree: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            output_log: None,
        }
    }

    /// ステータスを変更
    pub fn set_status(&mut self, status: TaskStatus) {
        self.status = status;
        match status {
            TaskStatus::InProgress => {
                if self.started_at.is_none() {
                    self.started_at = Some(Utc::now());
                }
            }
            TaskStatus::Done | TaskStatus::Cancelled => {
                self.completed_at = Some(Utc::now());
            }
            _ => {}
        }
    }

    /// エージェントをアサイン
    pub fn assign_agent(&mut self, agent: impl Into<String>, branch: impl Into<String>) {
        self.agent = Some(agent.into());
        self.branch = Some(branch.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_creation() {
        let task = Task::new("Test task", "Description");
        assert!(task.id.starts_with("task-"));
        assert_eq!(task.title, "Test task");
        assert_eq!(task.status, TaskStatus::Todo);
    }

    #[test]
    fn test_status_transition() {
        let mut task = Task::new("Test", "");
        assert!(task.started_at.is_none());

        task.set_status(TaskStatus::InProgress);
        assert!(task.started_at.is_some());
        assert!(task.completed_at.is_none());

        task.set_status(TaskStatus::Done);
        assert!(task.completed_at.is_some());
    }
}
