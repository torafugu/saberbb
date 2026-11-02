# 回転モデルで狙い位置と予定軸を分離する診断

## 目的と条件

上下対称の直線運動対照で、予定軸の高さ依存が追加のVLA下降を生むことを確認した。
今回は元の回転モデルへ戻り、同じ球・予測位置・予測時刻・観測誤差で予定軸の規則を比較する。
本体の打球結果・打者設定やDB形式を変更しない。

| axis_model | 予定軸の規則 |
|---|---|
| height_dependent | 従来の予測高さから求める10～60°の規則 |
| stance_30deg | 診断設定の基準角30°。狙い高さから独立 |

基準角は`--stance-angles-deg`で指定する。既定の30°は従来の高さ規則で
ストライクゾーン中央に対応する値を使った出発点であり、実測した打者の構えではない。
打者ごとの基準軸は現行スナップショットにないため、今回の設定は全打者で共通。
この値を最適値・本体の採用値とみなさない。

予定軸と狙い位置を分離するので、stance条件でも予測位置・時刻は変わる。
狙い位置をゾーン中央へ戻したり、真の到達高さで軸を選んだりはしない。

## 予測と計画の共有

前回の`observation_update_probe.py`と同じ関数で、次の5予測条件を作る。

- prior_only：既存expected_ballの速度・加速度と確定時点の正確な位置。
- observed_sigma_0.002 / 0.005 / 0.01：過去3観測からの推定更新。
- exact_observation_control：誤差なし3観測からの対照。

game_id/count_seqから作る標準誤差サンプル、batting_eyeの観測誤差倍率、観測時刻は前回と同じ。
予測を一度だけ作り、各予定軸モデルへ共有する。モデル間で観測を再抽選しない。
基準角の変更だけで予測誤差が改善したと主張する診断ではない。

各モデルで`plan_swing()`の予定角だけを変え、同じ回転計画生成を使う。
予定attack angle・バット速度の設定値と保存済み実行角度誤差は共通。
実行角度誤差は旧予測コースで保存された値で、更新後のコース・基準角から再計算しない。
したがって独立させる対象は予定軸であり、実行誤差の元のコース依存まで除いたわけではない。

```text
predicted_target, predicted_time = shared_forecast
planned_axis_angle = angle_from(predicted_height) OR explicit_stance_angle
plan = existing_plan_swing(shared_batter_config, predicted_target, planned_axis_angle)
```

元のFrameMotionで支点・角速度・接触点速度を扱い、モデルごとに接触を探索し直す。
予定軸を変えると、整合する支点・角速度・実行後アンカーのずれ・実行後の基準速度も変わりうる。
これは全計画としての候補比較であり、法線だけを変える比較や前段の速度固定対照ではない。
実行角度誤差による位置のずれの大きさに加え、従来モデルからの実行アンカー位置差
`plan_anchor_change_m`と基準速度差`reference_velocity_change_ms`も出力する。

半長0.16m、有効半径0.07m、e=0.5、M=0.6kg、探索±60ms・許容誤差1e-6m・上限4000反復は共通。
球は従来と同じ局所的な定速度近似。予定時刻は共有予測から決め、旧timing_offsetを加えない。
旧ContactTypeは結果確認用の分類で、予定軸・衝突計算を決める入力ではない。

## 実行

追加の試合生成・DB移行は不要。24958～25317の既存DBを使う例：

```sh
python3 tools/diagnostics/stance_axis_probe.py --db "/path/to/saberbb.db" --first-game-id 24958 --last-game-id 25317 --stance-angles-deg 30 --levers-m 0.9 --output-dir ../saberbb_stance_axis
```

出力先は未作成ディレクトリ。DBは読み取り専用・query_onlyのトランザクションで開く。
既存の予測・観測履歴がない行はstatusへ欠損理由を記録し、補完しない。
上記なら5予測×2軸×1レバー長＝1球10試行、48,562球なら485,620試行。
`--levers-m`省略時は0.6/0.9/1.2mの30試行。
基準角の感度は`--stance-angles-deg 20 30 40`等で比較できるが、まず30°の対照を評価する。

## 出力と分母

| ファイル | 内容 |
|---|---|
| stance_axis_coverage.csv | 全キー、有効履歴、1球の試行数・総試行数 |
| stance_axis_status.csv | 有効履歴・欠損・不正の件数 |
| stance_axis_checks.csv | 誤差なし予測、共有する目標・時刻・予定速度、回転計画のアンカー、鉛直速度分解の残差 |
| stance_axis_summary.csv | 予測条件×予定軸×レバー長の接触・衝突状態、EV/HLA/VLA・ファウル方向率等 |
| stance_axis_pairs.csv | 同じ予測・同じ球で双方が力積正の結果比較 |
| stance_axis_transitions.csv | 高さ依存軸→基準軸の探索・衝突状態の遷移 |
| stance_axis_metadata.json | 基準角・予測・回転モデル・係数の仮定 |

scope=all_swingsは全診断キー、recorded_batting_onlyはUnrecordedBattingを除く。
両scopeを合算しない。z_error_band=allは全球、他の帯は予測Z−実際Zの誤差帯。
allと帯域別も合算しない。予測誤差帯は同じ予測なら予定軸間で同じ球集合。
異なる予測条件の同じ帯名は同じ球集合とは限らない。

summaryのcontact_pctは幾何のある接触件数/statesで、ライブのContact%ではない。
未収束・開始時重複・非接近は別に数える。EV/HLA/VLA・鉛直速度の平均は力積正の集合。
法線・法線速度比・接触点速度は接触幾何がある集合。
予定角・実行位置ずれ・従来からのアンカー/基準速度差は全試行の平均。
ファウル方向率は発射方向の集計で、試合の最終結果ではない。
HLAの平均は算術平均なので、方向差の評価はペアの折り返した差とファウル方向率を使う。

pairsのbeforeはheight_dependent、afterはaxis_modelに示す基準軸モデル。
両条件とも力積正の同一球に限定し、各変数の有効ペア件数を示す。
HLA差は±180°へ折り返す。summaryの平均には接触した集合の違いがあるため、pairsと遷移を併用する。

## 確認順

1. coverage/status/checksで入力の充足・共有条件と計画の整合を確認。
2. z_error_band=all、height_dependentが前回の観測更新診断を再現するか確認。
3. 各予測条件で基準軸にしたときの接触・非接触の入れ替わりと未収束を確認。
4. 同一球のpairsでVLAとHLA・ファウル方向率がどう変わるか確認。
5. Z誤差帯ごとに、低め/高めを狙った球の方向の偏りがどう変わるか確認。
6. EV・強い打球比率が改善した方向分布と両立するか確認。

基準角をContact率や平均VLAへ合わせ込まず、予定軸を独立に持つ設計の影響を評価する。
この診断の結果だけで基準角を校正できたとはみなさず、本体適用は別に判断する。

## 検証

従来の高さ依存モデルが前回の観測更新結果を再現すること、同じ角度では同じ計画になること、
基準軸は目標の高さ・時刻の変更で変わらず狙い位置・時刻は追従すること、左右鏡映、
ペア/遷移の分母・非接触・空範囲・不正引数・DBのSHA-256不変性を検証する。

```sh
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
python3 -m unittest discover -s tests/sql -p 'test_*.py'
```

実行後はmetadata以外の6つのCSVを添付する。全試行明細・JSONLは新たに生成しない。
