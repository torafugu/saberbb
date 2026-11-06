# SaberBB Wiki

2026-11-06更新（実装基準: `c96ef1a`）。`main` の実装を基準に、ドメインモデル・通常試合の処理・診断経路を説明します。

通常試合には2ストライク防御、75%時間位置ずれ、符号付きHLA、10ms空中捕球探索を使用します。Player/Manager由来Intentと有限バット衝突応答は診断記録で、通常打球・得点への接続はまだ行いません。

- [プレイヤーパラメータ](player_parameters.md)
- [プレイヤー生成](player_generation.md)
- [投球パラメータ](pitching_parameters.md)
- [打撃パラメータ](batting_parameters.md)
- [投球ロジック](pitching_logic.md)
- [投手・捕手の配球統合](pitch_call_decision.md)
- [投球の物理](pitching_physics.md)
- [打撃ロジック](batting_logic.md)
- [打球の物理](batting_physics.md)
- [打撃戦略・選手の傾向・監督](batting_strategy.md)
- [守備ロジック](fielding_logic.md)
- [診断・再現・holdout](diagnostics.md)

[English](../en-US/index.md) · [詳細な診断仕様](../../)
