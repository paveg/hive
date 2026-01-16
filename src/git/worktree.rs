use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

/// git worktree を管理
pub struct WorktreeManager {
    /// リポジトリのルートパス
    repo_root: PathBuf,
    /// worktree を作成するディレクトリ
    worktree_dir: PathBuf,
    /// ブランチのプレフィックス
    branch_prefix: String,
}

impl WorktreeManager {
    pub fn new(repo_root: PathBuf, hive_dir: PathBuf) -> Self {
        let worktree_dir = hive_dir.join("worktrees");
        std::fs::create_dir_all(&worktree_dir).ok();

        Self {
            repo_root,
            worktree_dir,
            branch_prefix: "hive".into(),
        }
    }

    /// タスク用の worktree を作成
    pub fn create(&self, task_id: &str) -> Result<PathBuf> {
        let branch_name = format!("{}/{}", self.branch_prefix, task_id);
        let worktree_path = self.worktree_dir.join(task_id);

        // すでに存在する場合はそのまま返す
        if worktree_path.exists() {
            return Ok(worktree_path);
        }

        // 新しいブランチで worktree を作成
        let output = Command::new("git")
            .args(["worktree", "add", "-b", &branch_name])
            .arg(&worktree_path)
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to execute git worktree add")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // ブランチが既に存在する場合は、既存ブランチで作成を試みる
            if stderr.contains("already exists") {
                let output = Command::new("git")
                    .args(["worktree", "add"])
                    .arg(&worktree_path)
                    .arg(&branch_name)
                    .current_dir(&self.repo_root)
                    .output()
                    .context("Failed to execute git worktree add with existing branch")?;

                if !output.status.success() {
                    bail!(
                        "Failed to create worktree: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            } else {
                bail!("Failed to create worktree: {}", stderr);
            }
        }

        // .claude/settings.json を設定（plansDirectory）
        self.setup_claude_settings(&worktree_path)?;

        Ok(worktree_path)
    }

    /// worktree を削除
    pub fn remove(&self, task_id: &str) -> Result<()> {
        let worktree_path = self.worktree_dir.join(task_id);

        if !worktree_path.exists() {
            return Ok(());
        }

        // worktree を削除
        let output = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&worktree_path)
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to execute git worktree remove")?;

        if !output.status.success() {
            bail!(
                "Failed to remove worktree: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    /// worktree のパスを取得
    pub fn get_path(&self, task_id: &str) -> PathBuf {
        self.worktree_dir.join(task_id)
    }

    /// worktree が存在するか確認
    pub fn exists(&self, task_id: &str) -> bool {
        self.worktree_dir.join(task_id).exists()
    }

    /// ブランチ名を取得
    pub fn get_branch_name(&self, task_id: &str) -> String {
        format!("{}/{}", self.branch_prefix, task_id)
    }

    /// Claude Code の設定ファイルを作成
    fn setup_claude_settings(&self, worktree_path: &PathBuf) -> Result<()> {
        let claude_dir = worktree_path.join(".claude");
        std::fs::create_dir_all(&claude_dir).context("Failed to create .claude directory")?;

        // plansDirectory を .hive/plans に向ける（相対パス）
        let settings = serde_json::json!({
            "plansDirectory": "../../plans"
        });

        let settings_path = claude_dir.join("settings.json");
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&settings).unwrap(),
        )
        .context("Failed to write .claude/settings.json")?;

        Ok(())
    }

    /// メインブランチとの差分を取得
    pub fn get_diff(&self, task_id: &str, base_branch: &str) -> Result<String> {
        let worktree_path = self.worktree_dir.join(task_id);

        let output = Command::new("git")
            .args(["diff", base_branch])
            .current_dir(&worktree_path)
            .output()
            .context("Failed to execute git diff")?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// 変更をマージ
    pub fn merge(&self, task_id: &str, target_branch: &str) -> Result<()> {
        let branch_name = self.get_branch_name(task_id);

        // メインリポジトリでマージ
        let output = Command::new("git")
            .args(["merge", &branch_name, "--no-ff", "-m"])
            .arg(format!("Merge {} via Hive", task_id))
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to execute git merge")?;

        if !output.status.success() {
            bail!(
                "Failed to merge: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }
}
