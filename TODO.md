# Hive TODO

## 完了済み
- [x] Phase 0: Rust環境セットアップ (devbox)
- [x] Phase 1: TUI基礎 - カンバンレイアウト表示
- [x] Phase 2: Task管理 - 構造体とJSON永続化
- [x] Phase 3: エージェントアサイン + git worktree作成

## 次のステップ
- [ ] 非同期エージェント実行 - 実際にclaude/geminiコマンドを実行してログ表示
- [ ] CLIサブコマンド - `hive init`, `hive task add`, `hive task list`
- [ ] Diff表示 - worktreeの変更をTUIで確認
- [ ] マージ機能 - Review完了後にメインブランチにマージ
- [ ] Planner/Executor分離 - Gemini(Plan) → Claude(実装)のオーケストレーション

## 将来の拡張
- [ ] タスク依存関係
- [ ] MCP連携
- [ ] カスタムエージェント追加
- [ ] テーマ対応
- [ ] エージェントパネル（右サイド）
