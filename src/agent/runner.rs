use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

/// Agent configuration
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}

impl AgentConfig {
    /// Claude Code configuration
    pub fn claude() -> Self {
        Self {
            name: "claude".into(),
            command: "claude".into(),
            args: vec!["-p".into(), "--dangerously-skip-permissions".into()],
        }
    }

    /// Gemini CLI configuration
    pub fn gemini() -> Self {
        Self {
            name: "gemini".into(),
            command: "gemini".into(),
            args: vec!["-y".into()],
        }
    }

    /// Codex configuration
    pub fn codex() -> Self {
        Self {
            name: "codex".into(),
            command: "codex".into(),
            args: vec![],
        }
    }

    /// Get configuration by name
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "claude" => Some(Self::claude()),
            "gemini" => Some(Self::gemini()),
            "codex" => Some(Self::codex()),
            _ => None,
        }
    }

    /// Get list of available agents
    #[allow(dead_code)]
    pub fn available_agents() -> Vec<&'static str> {
        vec!["claude", "gemini", "codex"]
    }
}

/// Agent execution status
#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatus {
    #[allow(dead_code)]
    Idle,
    Running,
    Completed,
    Failed(String),
}

/// Running agent information
#[allow(dead_code)]
pub struct RunningAgent {
    pub task_id: String,
    pub config: AgentConfig,
    pub status: AgentStatus,
    pub output_lines: Vec<String>,
    child: Option<Child>,
}

/// Agent execution manager
pub struct AgentRunner {
    /// Running agents (task_id -> RunningAgent)
    pub agents: HashMap<String, RunningAgent>,
    /// Log directory
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

    /// Start agent
    pub async fn start(
        &mut self,
        task_id: &str,
        config: AgentConfig,
        working_dir: PathBuf,
        prompt: &str,
    ) -> Result<mpsc::Receiver<String>> {
        // Channel to receive output
        let (tx, rx) = mpsc::channel::<String>(100);

        // Add prompt to arguments
        let mut args = config.args.clone();
        args.push(prompt.to_string());

        // Start process
        let mut child = Command::new(&config.command)
            .args(&args)
            .current_dir(&working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context(format!("Failed to start {}", config.name))?;

        // Read stdout asynchronously
        if let Some(stdout) = child.stdout.take() {
            let tx_clone = tx.clone();
            let _task_id_clone = task_id.to_string();
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
                    // Write to log file
                    if let Some(ref mut file) = log_file {
                        use tokio::io::AsyncWriteExt;
                        let _ = file.write_all(format!("{}\n", line).as_bytes()).await;
                    }
                    // Send to channel
                    if tx_clone.send(line).await.is_err() {
                        break;
                    }
                }
                drop(tx_clone);
            });
        }

        // Handle stderr similarly
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

        // Register running agent
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

    /// Stop agent
    pub async fn stop(&mut self, task_id: &str) -> Result<()> {
        if let Some(agent) = self.agents.get_mut(task_id) {
            if let Some(mut child) = agent.child.take() {
                child.kill().await.context("Failed to kill process")?;
                agent.status = AgentStatus::Failed("Stopped by user".into());
            }
        }
        Ok(())
    }

    /// Get agent status
    #[allow(dead_code)]
    pub fn get_status(&self, task_id: &str) -> Option<&AgentStatus> {
        self.agents.get(task_id).map(|a| &a.status)
    }

    /// Get count of running agents
    pub fn running_count(&self) -> usize {
        self.agents
            .values()
            .filter(|a| a.status == AgentStatus::Running)
            .count()
    }

    /// Check completion and update status
    #[allow(dead_code)]
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
                        Ok(None) => {} // Still running
                        Err(e) => {
                            agent.status = AgentStatus::Failed(e.to_string());
                            agent.child = None;
                        }
                    }
                }
            }
        }
    }

    /// Check completion for specific task (sync version)
    pub fn check_task_completion(&mut self, task_id: &str) -> Option<AgentStatus> {
        if let Some(agent) = self.agents.get_mut(task_id) {
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
                        Ok(None) => {} // Still running
                        Err(e) => {
                            agent.status = AgentStatus::Failed(e.to_string());
                            agent.child = None;
                        }
                    }
                }
            }
            return Some(agent.status.clone());
        }
        None
    }
}
