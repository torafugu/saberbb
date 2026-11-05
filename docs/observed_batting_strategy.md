# 監督・コーチと打者の戦略を分け、観測更新後の計画へ接続する

投手側の`PitchingStrategy`と選手側の具体化を分ける構成に合わせ、打者側も次の責務を分離する。

| 層 | 型・処理 | 担当する判断 |
| --- | --- | --- |
| 監督・コーチ | BattingStrategy | 打席の目的・チームの優先事項 |
| 打者 | BattingPreferences → BattingIntentDecision | 自分の打球意図と、指示を受けた最終意図 |
| 観測・予測 | observation_update | 観測履歴から球の到達位置・時刻を推定 |
| 計画 | BattedBallIntent → plan_intended_swing | 予測投球高に応じた狙い、姿勢・速度・開始姿勢 |

監督・コーチがmm単位の狙い点を選ぶ構成にはしない。観測精度・打者の技術と、チームの目的も混同しない。
この段階では両レベルの方針を外部から明示設定し、カウント・点差・走者・打者タイプからの自動戦術選択は加えない。
投手側の候補評価の構造を参考にするが、投手・捕手間の交渉処理や未検証の重みをそのまま打者へ持ち込む変更ではない。

## 初期の方針と打者判断

| 監督・コーチの方針 | 選択する打球意図 | 判断理由 |
| --- | --- | --- |
| LetBatterDecide | 打者のpreferred_intent | batter_preference |
| PutBallInPlay | ContactFirst | manager_contact_priority |
| SeekLineDrive | LineDrive | manager_line_drive_priority |

初版ではチームから目的が明示されたときはその目的を優先し、任せる場合は打者の選好を使う。
指示を優先した場合も元の打者選好を記録へ残す。将来、選手の能力や予測の不確実性を踏まえた候補評価を追加できる境界とする。
この対応は診断用の初期ルールであり、得点期待値から校正した最適戦術や選手の実際の好みの推定ではない。

`GameState.batting_strategy_settings`でチーム別の指示と選手別の選好を指定できる。

```rust
use saberbb::domain::strategy::batting_strategy::{
    BattingStrategy, BattingPreferences, BattedBallIntent,
};

game_state.batting_strategy_settings.away_strategy = BattingStrategy::LetBatterDecide;
game_state.batting_strategy_settings.home_strategy = BattingStrategy::PutBallInPlay;
game_state.batting_strategy_settings.player_preferences.insert(
    42,
    BattingPreferences { preferred_intent: BattedBallIntent::LineDrive },
);
```

これはAPIの設定例で、選手IDは実際の対象へ置き換える。既定は両チームLetBatterDecide、未指定の打者はContactFirst。
表の例ではビジター攻撃時の42番はLineDrive、ホーム攻撃時は指示が優先されContactFirstになる。
UIへの設定画面や方針のDB永続化は今回の範囲に含めない。

`process_count()`の投球生成前に、攻撃側の方針と打者IDに対応する選好から判断を固定する。
打球結果や実際の接触を見て方針を変えない。判断・観測更新・計画は現在すべて診断用であり、ライブの打球結果へは適用しない。

## 観測更新のRust移植

既存Python診断の`observed_sigma_0.005`と同じ次の処理を移植した。

1. prior flight clockの0.2／0.4／0.6時点に収集済みの位置履歴を検証する。
2. sigma 0.005 m、`exp(-0.5 * clamp(batting_eye, -4, 4))`の感度仮説で観測ノイズを加える。
3. 過去3点の二次補間から速度・加速度を推定する。Y加速度は0へ射影し、Y速度は全観測区間から推定する。
4. 最後の観測と推定速度・加速度だけから到達位置・時刻を予測する。
5. 更新後の予測を両意図の固定計画へ渡す。角度実行誤差はPythonと同じくprior側の保存値を保持する。

`update_forecast()`は観測・sigma・eye・標準ノイズだけを受け取り、実際の本塁面到達位置・時刻を引数に持たない。
観測の収集は従来どおりシミュレーション軌道を使うが、推定器・計画器へ将来の真値を渡さない。

ノイズは既存Pythonの`standard_noise(game_id, count_seq)`を再現する。
SHA256の投球キーと、整数seedに対応するMT19937・53bit一様乱数・キャッシュ付きGaussian生成を独立した診断用生成器で実装した。
試合本体のRandomProviderは呼ばない。同じキーならPythonと同じノイズになり、能力比較でも同じ標準ノイズを使える。
sha2は既存lock内の0.10.9を直接依存へ加えた。

## 保存される診断記録

既存の`intent_plans`（prior予測の比較計画）は保持し、新しい任意フィールド
`state_json.observed_intent_plans`へ更新後の記録を追加する。DB移行は不要。

- game_id/count_seq、sigma、eye、標準ノイズ。
- 監督・コーチの方針、打者の選好、選択された意図、判断理由。
- ノイズ付き観測、推定速度・加速度、更新後の予測。
- 更新後の予測から生成した2意図の比較計画と開始姿勢・時計不成立理由。

選択された意図は`decision.selected_intent`、対応する計画は`intent_plans.plans`からintentが一致する要素で参照できる。
比較のため両意図を保存するが、選択済みの意図と候補を混同しない。
観測更新が失敗した場合はinvalid_reasonを保存し、priorの計画へ黙ってフォールバックしない。
時計不成立の場合も既存の計画診断と同じ区別を維持し、空振りに数えない。

## 実データでの照合

取り込み後、新しい試合を生成・保存して実行する。DBパスは環境変数`SABERBB_DB`に設定する。以下のIDは例で、今回生成した範囲へ置き換える。

```sh
python3 tools/diagnostics/rust_observed_intent_audit.py --first-game-id 26164 --last-game-id 26173 --output-dir ../saberbb_rust_observed_intent_audit
```

DBは読み取り専用。既存のstate_jsonからPythonで標準ノイズ・観測更新・予測・計画を再生成して照合する。
チーム方針と打者選好に対する選択・理由の整合性も検査する。接触探索・衝突再計算は不要。
元の観測更新キャッシュではなく、新しいRust診断記録を保存したDBを入力にする。
古い記録には新フィールドがないため、missingとして数え、実際の終点から補完しない。

共有する3ファイル：

- `rust_observed_intent_coverage.csv`
- `rust_observed_intent_checks.csv`
- `rust_observed_intent_metadata.json`

details CSVには意図、selected、strategy、preferred_intent、decision_reasonも保存する。必要時のみ共有する。
既定設定で全員ContactFirstが選ばれても異常ではなく、両候補の計画は引き続き照合される。
現在の監査は記録された指示・選好に対する解決ルールを確認するもので、外部の監督設定データまで独立検証するものではない。

## テスト

```sh
cargo test --locked --lib observation_update
cargo test --locked --lib intent_levels_tests
cargo test --locked --lib contact_state
python3 -m unittest discover -s tests/diagnostics -p 'test_rust_observed_intent_audit.py'
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
```

共通7ケースでノイズと観測更新を照合し、eyeの大小・clamp・sigma 0・最大ゲームID／カウントを含めた。
打者に任せる場合と指示優先、ホーム／ビジターと選手別設定の分離、新旧JSON互換、既存状態の不変性、
DB読み取り専用、観測・決定記録の改変拒否、更新失敗時の非フォールバックを確認する。

センサーのsigmaと能力への感度は未校正の仮説である。今回も打球性能を改善したという検証ではなく、
同じ入力モデルをRustへ移し、戦略階層を分けて計画を記録できることの検証にとどめる。

開始姿勢からの実行・幾何学的接触の検証は[実行・接触診断](rust_execution_contact.md)を参照。
