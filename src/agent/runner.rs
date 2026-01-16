use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

/// エージェントの設定
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}

impl AgentConfig {
    /// Claude Code の設定
    pub fn claude() -> Self {
        Self {
            name: "claude".into(),
            command: "claude".into(),
            args: vec!["-p".into(), "--dangerously-skip-permissions".into()],
        }
    }

    /// Gemini CLI の設定
    pub fn gemini() -> Self {
        Self {
            name: "gemini".into(),
            command: "gemini".into(),
            args: vec!["-y".into()],
        }
    }

    /// Codex の設定
    pub fn codex() -> Self {
        Self {
            name: "codex".into(),
            command: "codex".into(),
            args: vec![],
        }
    }

    /// 名前から設定を取得
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "claude" => Some(Self::claude()),
            "gemini" => Some(Self::gemini()),
            "codex" => Some(Self::codex()),
            _ => None,
        }
    }

    /// 利用可能なエージェント一覧
    pub fn available_agents() -> Vec<&'static str> {
        vec!["claude", "gemini", "codex"]
    }
}

/// エージェントの実行状態
#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatus {
    Idle,
    Running,
    Completed,
    Failed(String),
}

/// 実行中のエージェント情報
pub struct RunningAgent {
    pub task_id: String,
    pub config: AgentConfig,
    pub status: AgentStatus,
    pub output_lines: Vec<String>,
    child: Option<Child>,
}

/// エージェント実行を管理
pub struct AgentRunner {
    /// 実行中のエージェント (task_id -> RunningAgent)
    pub agents: HashMap<String, RunningAgent>,
    /// ログディレクトリ
    log_dir: PathBuf,
}

impl AgentRunner {
    pub fn new(hive_dir: PathBuf) -> Self {
        let log_dir = hive_dir.join("logs");
        std::fs::create_dir_all(&log_dir).ok();

        Self {
            agents: HashMap::new(),
            log_dir,
        }
    }

    /// エージェントを起動
    pub async fn start(
        &mut self,
        task_id: &str,
        config: AgentConfig,
        working_dir: PathBuf,
        prompt: &str,
    ) -> Result<mpsc::Receiver<String>> {
        // 出力を受け取るチャンネル
        let (tx, rx) = mpsc::channel::<String>(100);

        // プロンプトを引数に追加
        let mut args = config.args.clone();
        args.push(prompt.to_string());

        // プロセス起動
        let mut child = Command::new(&config.command)
            .args(&args)
            .current_dir(&working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context(format!("Failed to start {}", config.name))?;

        // stdout を非同期で読み取り
        if let Some(stdout) = child.stdout.take() {
            let tx_clone = tx.clone();
            let task_id_clone = task_id.to_string();
            let log_path = self.log_dir.join(format!("{}.log", task_id));

            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                let mut log_file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_path)
                    .await
                    .ok();

                while let Ok(Some(line)) = lines.next_line().await {
                    // ログファイルに書き込み
                    if let Some(ref mut file) = log_file {
                        use tokio::io::AsyncWriteExt;
                        let _ = file.write_all(format!("{}\n", line).as_bytes()).await;
                    }
                    // チャンネルに送信
                    if tx_clone.send(line).await.is_err() {
                        break;
                    }
                }
                drop(tx_clone);
            });
        }

        // stderr も同様に処理
        if let Some(stderr) = child.stderr.take() {
            let tx_clone = tx;
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = tx_clone.send(format!("[stderr] {}", line)).await;
                }
            });
        }

        // 実行中エージェントを登録
        let running = RunningAgent {
            task_id: task_id.to_string(),
            config,
            status: AgentStatus::Running,
            output_lines: Vec::new(),
            child: Some(child),
        };
        self.agents.insert(task_id.to_string(), running);

        Ok(rx)
    }

    /// エージェントを停止
    pub async fn stop(&mut self, task_id: &str) -> Result<()> {
        if let Some(agent) = self.agents.get_mut(task_id) {
            if let Some(mut child) = agent.child.take() {
                child.kill().await.context("Failed to kill process")?;
                agent.status = AgentStatus::Failed("Stopped by user".into());
            }
        }
        Ok(())
    }

    /// エージェントの状態を取得
    pub fn get_status(&self, task_id: &str) -> Option<&AgentStatus> {
        self.agents.get(task_id).map(|a| &a.status)
    }

    /// 実行中のエージェント数
    pub fn running_count(&self) -> usize {
        self.agents
            .values()
            .filter(|a| a.status == AgentStatus::Running)
            .count()
    }

    /// 完了をチェックして状態を更新
    pub async fn check_completion(&mut self) {
        for agent in self.agents.values_mut() {
            if agent.status == AgentStatus::Running {
                if let Some(ref mut child) = agent.child {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            agent.status = if status.success() {
                                AgentStatus::Completed
                            } else {
                                AgentStatus::Failed(format!("Exit code: {:?}", status.code()))
                            };
                            agent.child = None;
                        }
                        Ok(None) => {} // まだ実行中
                        Err(e) => {
                            agent.status = AgentStatus::Failed(e.to_string());
                            agent.child = None;
                        }
                    }
                }
            }
        }
    }
}
