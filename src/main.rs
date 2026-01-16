mod agent;
mod git;
mod task;

use std::io;
use std::path::PathBuf;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use agent::AgentConfig;
use git::WorktreeManager;
use task::{Task, TaskStatus, TaskStore};

/// 入力モード
#[derive(Debug, Clone, PartialEq)]
enum InputMode {
    Normal,
    /// タスク作成中 (タイトル入力)
    NewTaskTitle,
    /// タスク作成中 (説明入力)
    NewTaskDescription,
    /// エージェント選択中
    SelectAgent,
}

/// アプリケーションの状態
struct App {
    /// タスクストア
    store: TaskStore,
    /// 全タスク
    tasks: Vec<Task>,
    /// 現在選択中のカラム
    selected_column: usize,
    /// 各カラムで選択中のタスクインデックス
    selected_task: [usize; 4],
    /// 入力モード
    input_mode: InputMode,
    /// 入力中のテキスト
    input_buffer: String,
    /// 作成中のタスクタイトル（一時保存）
    pending_title: String,
    /// ステータスメッセージ
    status_message: Option<String>,
    /// Worktree マネージャー
    worktree_manager: WorktreeManager,
    /// エージェント選択リスト
    agent_list: Vec<&'static str>,
    /// 選択中のエージェントインデックス
    selected_agent: usize,
}

impl App {
    fn new() -> anyhow::Result<Self> {
        let hive_dir = PathBuf::from(".hive");
        let store = TaskStore::new(".")?;
        let tasks = store.load()?;
        let worktree_manager = WorktreeManager::new(PathBuf::from("."), hive_dir);

        Ok(Self {
            store,
            tasks,
            selected_column: 0,
            selected_task: [0; 4],
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            pending_title: String::new(),
            status_message: None,
            worktree_manager,
            agent_list: AgentConfig::available_agents(),
            selected_agent: 0,
        })
    }

    /// 指定カラムのタスクを取得
    fn tasks_in_column(&self, column: usize) -> Vec<&Task> {
        let status = match column {
            0 => TaskStatus::Todo,
            1 => TaskStatus::InProgress,
            2 => TaskStatus::Review,
            3 => TaskStatus::Done,
            _ => return vec![],
        };
        self.tasks.iter().filter(|t| t.status == status).collect()
    }

    /// 現在選択中のタスクを取得
    fn selected_task(&self) -> Option<&Task> {
        let tasks = self.tasks_in_column(self.selected_column);
        tasks.get(self.selected_task[self.selected_column]).copied()
    }

    /// 現在選択中のタスクを可変で取得
    fn selected_task_mut(&mut self) -> Option<&mut Task> {
        let status = match self.selected_column {
            0 => TaskStatus::Todo,
            1 => TaskStatus::InProgress,
            2 => TaskStatus::Review,
            3 => TaskStatus::Done,
            _ => return None,
        };
        let idx = self.selected_task[self.selected_column];
        self.tasks
            .iter_mut()
            .filter(|t| t.status == status)
            .nth(idx)
    }

    fn move_left(&mut self) {
        if self.selected_column > 0 {
            self.selected_column -= 1;
            self.clamp_selection();
        }
    }

    fn move_right(&mut self) {
        if self.selected_column < 3 {
            self.selected_column += 1;
            self.clamp_selection();
        }
    }

    fn move_up(&mut self) {
        let col = self.selected_column;
        if self.selected_task[col] > 0 {
            self.selected_task[col] -= 1;
        }
    }

    fn move_down(&mut self) {
        let col = self.selected_column;
        let count = self.tasks_in_column(col).len();
        if count > 0 && self.selected_task[col] < count - 1 {
            self.selected_task[col] += 1;
        }
    }

    /// 選択インデックスを範囲内に収める
    fn clamp_selection(&mut self) {
        let col = self.selected_column;
        let count = self.tasks_in_column(col).len();
        if count == 0 {
            self.selected_task[col] = 0;
        } else if self.selected_task[col] >= count {
            self.selected_task[col] = count - 1;
        }
    }

    /// タスク追加開始
    fn start_new_task(&mut self) {
        self.input_mode = InputMode::NewTaskTitle;
        self.input_buffer.clear();
        self.status_message = Some("Enter task title (ESC to cancel)".into());
    }

    /// エージェント選択開始
    fn start_assign_agent(&mut self) {
        if self.selected_task().is_some() {
            self.input_mode = InputMode::SelectAgent;
            self.selected_agent = 0;
            self.status_message = Some("Select agent (Enter to confirm, ESC to cancel)".into());
        } else {
            self.status_message = Some("No task selected".into());
        }
    }

    /// エージェントをアサインしてworktreeを作成
    fn assign_agent(&mut self) -> anyhow::Result<()> {
        let agent_name = self.agent_list[self.selected_agent];

        // タスク情報を取得
        let (task_id, task_title) = {
            let task = self.selected_task().ok_or_else(|| anyhow::anyhow!("No task selected"))?;
            (task.id.clone(), task.title.clone())
        };

        // Worktree を作成
        let worktree_path = self.worktree_manager.create(&task_id)?;
        let branch_name = self.worktree_manager.get_branch_name(&task_id);

        // タスクを更新
        if let Some(task) = self.selected_task_mut() {
            task.agent = Some(agent_name.to_string());
            task.branch = Some(branch_name.clone());
            task.worktree = Some(worktree_path.to_string_lossy().to_string());
            task.set_status(TaskStatus::InProgress);
        }

        self.store.save(&self.tasks)?;
        self.input_mode = InputMode::Normal;
        self.status_message = Some(format!(
            "Assigned {} to '{}' (branch: {})",
            agent_name, task_title, branch_name
        ));

        Ok(())
    }

    /// タスクを次のステータスに移動
    fn move_task_forward(&mut self) -> anyhow::Result<()> {
        if let Some(task) = self.selected_task_mut() {
            let new_status = match task.status {
                TaskStatus::Todo => TaskStatus::InProgress,
                TaskStatus::InProgress => TaskStatus::Review,
                TaskStatus::Review => TaskStatus::Done,
                TaskStatus::Done => return Ok(()),
                TaskStatus::Cancelled => return Ok(()),
            };
            task.set_status(new_status);
            self.store.save(&self.tasks)?;
            self.status_message = Some(format!("Moved to {}", new_status.display_name()));
            self.clamp_selection();
        }
        Ok(())
    }

    /// タスクを前のステータスに移動
    fn move_task_backward(&mut self) -> anyhow::Result<()> {
        if let Some(task) = self.selected_task_mut() {
            let new_status = match task.status {
                TaskStatus::Todo => return Ok(()),
                TaskStatus::InProgress => TaskStatus::Todo,
                TaskStatus::Review => TaskStatus::InProgress,
                TaskStatus::Done => TaskStatus::Review,
                TaskStatus::Cancelled => return Ok(()),
            };
            task.set_status(new_status);
            self.store.save(&self.tasks)?;
            self.status_message = Some(format!("Moved to {}", new_status.display_name()));
            self.clamp_selection();
        }
        Ok(())
    }

    /// タスクを削除
    fn delete_task(&mut self) -> anyhow::Result<()> {
        if let Some(task) = self.selected_task() {
            let id = task.id.clone();
            // Worktree があれば削除
            if task.worktree.is_some() {
                let _ = self.worktree_manager.remove(&id);
            }
            self.tasks.retain(|t| t.id != id);
            self.store.save(&self.tasks)?;
            self.status_message = Some("Task deleted".into());
            self.clamp_selection();
        }
        Ok(())
    }

    /// 入力を処理
    fn handle_input(&mut self, c: char) {
        self.input_buffer.push(c);
    }

    /// バックスペース
    fn handle_backspace(&mut self) {
        self.input_buffer.pop();
    }

    /// 入力確定
    fn confirm_input(&mut self) -> anyhow::Result<()> {
        match self.input_mode {
            InputMode::NewTaskTitle => {
                if !self.input_buffer.is_empty() {
                    self.pending_title = self.input_buffer.clone();
                    self.input_buffer.clear();
                    self.input_mode = InputMode::NewTaskDescription;
                    self.status_message = Some("Enter description (Enter to skip)".into());
                }
            }
            InputMode::NewTaskDescription => {
                let task = Task::new(&self.pending_title, &self.input_buffer);
                self.tasks.push(task);
                self.store.save(&self.tasks)?;
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
                self.pending_title.clear();
                self.status_message = Some("Task created!".into());
            }
            InputMode::SelectAgent => {
                self.assign_agent()?;
            }
            InputMode::Normal => {}
        }
        Ok(())
    }

    /// 入力キャンセル
    fn cancel_input(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
        self.pending_title.clear();
        self.status_message = None;
    }

    /// エージェント選択を上に移動
    fn agent_up(&mut self) {
        if self.selected_agent > 0 {
            self.selected_agent -= 1;
        }
    }

    /// エージェント選択を下に移動
    fn agent_down(&mut self) {
        if self.selected_agent < self.agent_list.len() - 1 {
            self.selected_agent += 1;
        }
    }
}

fn main() -> anyhow::Result<()> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let mut app = App::new()?;

    loop {
        terminal.draw(|frame| ui(frame, &app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match &app.input_mode {
                        InputMode::Normal => match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('h') | KeyCode::Left => app.move_left(),
                            KeyCode::Char('l') | KeyCode::Right => app.move_right(),
                            KeyCode::Char('k') | KeyCode::Up => app.move_up(),
                            KeyCode::Char('j') | KeyCode::Down => app.move_down(),
                            KeyCode::Char('n') => app.start_new_task(),
                            KeyCode::Char('a') => app.start_assign_agent(),
                            KeyCode::Char('m') | KeyCode::Tab => {
                                app.move_task_forward()?;
                            }
                            KeyCode::Char('M') | KeyCode::BackTab => {
                                app.move_task_backward()?;
                            }
                            KeyCode::Char('x') | KeyCode::Delete => {
                                app.delete_task()?;
                            }
                            _ => {}
                        },
                        InputMode::NewTaskTitle | InputMode::NewTaskDescription => match key.code {
                            KeyCode::Enter => app.confirm_input()?,
                            KeyCode::Esc => app.cancel_input(),
                            KeyCode::Backspace => app.handle_backspace(),
                            KeyCode::Char(c) => app.handle_input(c),
                            _ => {}
                        },
                        InputMode::SelectAgent => match key.code {
                            KeyCode::Enter => app.confirm_input()?,
                            KeyCode::Esc => app.cancel_input(),
                            KeyCode::Char('k') | KeyCode::Up => app.agent_up(),
                            KeyCode::Char('j') | KeyCode::Down => app.agent_down(),
                            _ => {}
                        },
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn ui(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);

    // ヘッダー
    let task_count = app.tasks.len();
    let header_text = format!(" HIVE - AI Agent Kanban  ({} tasks) ", task_count);
    let header = Paragraph::new(header_text)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, main_layout[0]);

    // カンバン
    let kanban_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(main_layout[1]);

    let columns = [
        ("📋 Todo", Color::Yellow),
        ("🔄 Progress", Color::Blue),
        ("👀 Review", Color::Magenta),
        ("✅ Done", Color::Green),
    ];

    for (i, ((title, color), col_area)) in columns.iter().zip(kanban_layout.iter()).enumerate() {
        let is_selected = i == app.selected_column;
        let tasks = app.tasks_in_column(i);

        let items: Vec<ListItem> = tasks
            .iter()
            .enumerate()
            .map(|(j, task)| {
                let style = if is_selected && j == app.selected_task[i] {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default()
                };
                // エージェントが割り当てられていれば表示
                let agent_icon = task
                    .agent
                    .as_ref()
                    .map(|a| match a.as_str() {
                        "claude" => " 🤖",
                        "gemini" => " ✨",
                        "codex" => " 🔮",
                        _ => "",
                    })
                    .unwrap_or("");
                ListItem::new(format!(" {}{} ", task.title, agent_icon)).style(style)
            })
            .collect();

        let border_style = if is_selected {
            Style::default().fg(*color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let list = List::new(items).block(
            Block::default()
                .title(format!("{} ({})", title, tasks.len()))
                .borders(Borders::ALL)
                .border_style(border_style),
        );

        frame.render_widget(list, *col_area);
    }

    // フッター
    let footer_text = match &app.input_mode {
        InputMode::Normal => app
            .status_message
            .as_deref()
            .unwrap_or(" [h/l] move  [j/k] select  [n]ew  [a]ssign  [m]ove→  [x]delete  [q]uit "),
        _ => app.status_message.as_deref().unwrap_or(""),
    };
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, main_layout[2]);

    // 入力モードならポップアップを表示
    match app.input_mode {
        InputMode::NewTaskTitle | InputMode::NewTaskDescription => {
            let popup_area = centered_rect(50, 20, area);
            frame.render_widget(Clear, popup_area);

            let title = match app.input_mode {
                InputMode::NewTaskTitle => "New Task - Title",
                InputMode::NewTaskDescription => "New Task - Description",
                _ => "",
            };

            let input = Paragraph::new(app.input_buffer.as_str())
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .title(title)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan)),
                );
            frame.render_widget(input, popup_area);
        }
        InputMode::SelectAgent => {
            let popup_area = centered_rect(40, 30, area);
            frame.render_widget(Clear, popup_area);

            let items: Vec<ListItem> = app
                .agent_list
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let icon = match *name {
                        "claude" => "🤖",
                        "gemini" => "✨",
                        "codex" => "🔮",
                        _ => "•",
                    };
                    let style = if i == app.selected_agent {
                        Style::default().bg(Color::Cyan).fg(Color::Black)
                    } else {
                        Style::default()
                    };
                    ListItem::new(format!(" {} {} ", icon, name)).style(style)
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title("Select Agent")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );
            frame.render_widget(list, popup_area);
        }
        InputMode::Normal => {}
    }
}

/// 中央寄せの矩形を計算
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
