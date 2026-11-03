# 投球ごとの実行状態と加減速を持つスイング診断

選手の基準スイング速度と、その投球で実行できた速度を分離する最初の段階として、保存済み投球を使う診断経路に実装する。従来の一定角速度を対照に残す。Rustの試合結果や選手能力値は変更せず、ここで比較してから既定モデルへの適用を判断する。

```sh
python3 tools/diagnostics/swing_execution_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_swing_execution \
  --variant observed_sigma_0.005_r0.9 \
  --gyro-sign 0
```

元データを作成したDBを `SABERBB_DB` に設定しておく。明示した `--db` も利用可能。選手再生成・試合再実行は不要だが、新しい軌道での接触探索は必要。

既定では、実行時間0.18秒、基準時刻が実行時間の中央、開始時刻の追加ずれ0秒とし、effort=1.0と0.85の2条件を比較する。これは校正済みの人間の速度曲線や、発生確率を表す分布ではない。各投球に同じ明示条件を適用し、感度を調べる。実際の到達位置や接触結果からeffortを決めたり、乱数で低速スイングを混ぜたりしない。

## 計画と実行ずれの分離（追加）

現在は[計画・実行分離モデル](swing_plan_execution.md)を使う。effort/--plan-effortsは意図した計画の強さを意味し、--execution-ratiosは同じ開始姿勢からの加速不足を表す。既定の実行比率1では従来の計画effort実験と一致する。以下の基準位置固定の説明は、実行比率1の計画対照に対応する。実行比率を変えた場合には到達位置を固定しない。

## 実行状態

`SwingExecution` は投球キー、保存計画の基準時刻、effort、duration、anchor_fraction、start_offsetを保持する。

- `reference_time_sec`: 保存した予測スイング計画のnominal時刻。実際の到達時刻から実行状態を選ばない。
- `effort`: 基準の並進速度と角速度の振幅に掛ける0より大きく1以下の係数。
- `duration_sec`: 速度パルスの長さ。
- `anchor_fraction`: 基準時刻がパルス内のどの位置にあるか。0.5がピーク。
- `start_offset_sec`: 正なら全スイング軌道が遅れる。位置と速度の両方に一度だけ適用する。

回転・並進ともに滑らかな加減速を持つ。パルスの前後では停止し、ピークで基準速度にeffortを掛けた値になる（anchor_fraction=0.5の場合）。

## 位置と速度の一致

実行開始からの正規化位相uに対し、速度係数を `g(u)=sin(pi*u)^2`（0<u<1）、外側は0とする。始点からの積分は

`F(u) = duration * (u/2 - sin(2*pi*u)/(4*pi))`

で、uを0〜1に制限する。保存基準時刻をq=0とし、`A(q)=F(q)-F(0)` を回転角と支点位置の両方に使う。

- 軸: `rotate(axis0, effort*omega0, A(q))`
- 支点: `pivot0 + effort*vp0*A(q)`
- 点位置: `pivot(q) + axis(q)*(lever+s)`
- 瞬間角速度: `effort*omega0*g(q)`
- 点速度: `effort*vp0*g(q) + cross(instantaneous_omega, axis(q)*(lever+s))`

実行比率1かつ開始ずれが0なら、基準時刻の位置・軸は保存計画と一致する。速度の変化は衝突前の軌道に反映し、接触後のEVを倍率で補正しない。努力値を変えるとパルス全体の移動量も変わるため、同じ開始姿勢やバックスイング量まで固定する比較ではない。

接触探索は球速とバット全点の最大速度から得た保守的な速度上限で進める。空間許容誤差1e-6m、反復上限4000、探索窓は保存値を維持。衝突応答にはピーク角速度ではなく接触時の瞬間角速度を渡す。加減速中のアクチュエータの仕事や衝突時間中の加速度変化はモデル化せず、衝突の瞬間だけ従来の凍結応答式を使う。

## 観測時刻との整合

観測更新条件では、計画に使用した最後の観測時刻を取得する。prior条件は保存予測の観測時刻を使う。構成した開始時刻がその時刻より前なら `not_evaluated:execution_starts_before_plan`、リリース前なら `not_evaluated:execution_starts_before_release` として保持し、探索しない。観測後の計画を過去へ遡って実行しないための制約。スイング中の再計画・操舵は今回扱わない。

これらは空振りに分類しない。summaryの `execution_clock_invalid_trials` に記録し、`evaluable_trials`、`evaluable_geometry_contacts`、`contact_fraction_of_evaluable_trials` では対照側からも同じ投球を除外する。

## 出力と分母

| ファイル | 内容 |
| --- | --- |
| `swing_execution_summary.csv` | scenario別の接触率、低EV/高EV、フェア方向率、平均EV/VLAなど |
| `swing_execution_pairs.csv` | 同じ投球で両条件の応答が成功した集合の比較 |
| `swing_execution_transitions.csv` | 接触/非接触/初期重複/未解決/時計不成立/応答未評価/方向の遷移 |
| `swing_execution_ev_vla.csv` | 全成功接触・フェア方向それぞれのEV/VLAグリッド |
| `swing_execution_checks.csv` | DB照合・基準姿勢・衝突応答の整合 |
| `swing_execution_coverage.csv` | 元投球数、実際の新規探索数、scenarioごとの採用理由 |
| `swing_execution_states.jsonl` | 各投球の実行状態、利用可能時刻、再構成用基準値、接触時の角速度/gain。時計不成立も理由付きで保存 |
| `swing_execution_metadata.json` | 条件、速度曲線、入力件数、回転の出典、範囲と制約 |

`orientation=saved_constant` が対照、`execution` が新モデル。scenario、scope、modelは重なるので合算しない。同じ対照が各scenarioに繰り返し出る。

`trials` は時計不成立も含む元投球数。既存集計互換の `contact_fraction` はその分母を使うため、接触率比較には **contact_fraction_of_evaluable_trials** を使う。これは対照/実行の両方で、時計条件が成立する同じ集合を分母にする。初期重複と探索未解決は別の状態としてtransitions/coverageで確認する。

EV/VLA平均とfair_fraction_of_successfulは各条件の成功接触の値で、接触する投球の入れ替わりを含む。共通集合での変化はpairsを使う。低EVは25m/s未満、高EVは45m/s以上。フェア方向は衝突直後の水平角による判定。ヒストグラムはcohort_contactsを分母とする非ゼロセルのみ。

保存済みの一定角速度側は再探索しない。応答は対照を投球ごとに1回、新モデルをscenarioごとに1回計算し、normal_profile/finite_mu0/finite_I0.00015_et0.4を比較。ゼロ回転・ゼロ遅延対照・接地後軌道は省略。全入力の監査結果はsource_audit/に残す。

実データでの改善や所要時間は未確認。結果依存で係数を選ぶ前に、低EVの増加とともに接触率や高EVを過度に下げていないか、時計不成立が増えていないかを確認する。実行後はsummary、pairs、transitions、ev_vla、checks、coverageのCSVとmetadata JSONを共有する。大きなstates JSONLは通常不要。

## 条件を変える例

```sh
python3 tools/diagnostics/swing_execution_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_swing_execution_phase \
  --efforts 1 \
  --duration-sec 0.18 \
  --anchor-fractions 0.35 0.5 0.65 \
  --start-offset-ms 0
```

実行時間内の基準位相の影響を、振りの強さとは別に比較する例。探索回数は選択投球数×時計が成立するscenario数に増える。
