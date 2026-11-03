# 計画するバット傾斜の固定小標本比較

接触姿勢の診断では、上向きの法線を持つ接触でも、バット軸が大きく傾いた条件でファウル方向が増えた。因果を切り分けるため、同じ投球集合に対して意図した傾斜だけを変え、各計画の開始姿勢から前向きに実行する。

## 実行

```sh
python3 tools/diagnostics/plan_tilt_sweep.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_plan_tilt_sample \
  --sample-size 2000 \
  --sample-seed 0 \
  --tilts-deg 0 15 30
```

DBは従来の `SABERBB_DB` を使う。`--db PATH` があれば優先する。入力は元の観測更新出力で、`observation_update_contacts.csv` と `observation_update_inputs.jsonl` が必要。同じデータを生成したDBを指定し、選手や試合を再生成しない。出力先は新しいフォルダを指定する。

既定variantは `observed_sigma_0.005_r0.9`、実行時間0.18秒、anchor_fraction=0.5、計画effort=1、実行比率=1、開始遅れ0。投球回転のgyro_signは0。期間・リーグの校正やStatcastへの自動フィットは行わない。0/15/30度は感度を見る任意の条件で、推奨角度ではない。

保存された高さ依存の傾斜を対照として必ず実行する。候補角度が3個なら、最大 `2000 × (対照1 + 候補3) = 8000` 回の接触探索。時計が成立しない投球は全条件で未評価とし探索しない。対照は投球ごとに一度だけ探索し、候補との各比較で再利用する。実データでの所要時間は未測定。

## 標本の固定

同じvariantの全投球を母集団とし、`SHA256("plan-tilt-v1:seed:game_id:count_seq")` の小さい順に選ぶ。キー以外の接触状態・EV・VLA・試合結果・時計成立は標本選択に使わない。ゲームの乱数は消費しない。選択したキーとハッシュは `plan_tilt_sample.csv` に保存する。

母集団・seed・標本数が同じなら入力の行順に依存せず同じ集合を選び、すべての傾斜で共有する。母集団が変われば同じseedでも集合が変わりうる。母集団が標本数より小さければ全件を使う。複数候補を別コマンドに分ける場合も、元データ・seed・標本数を揃える。

最初の結果を見て改善する条件があれば、別seedの小標本で確かめ、必要な条件だけ全件へ広げる。全件は `--sample-size 0`。この場合は接触探索が増えるので、既定の小標本で傾向を確認してから明示的に指定する。

```sh
python3 tools/diagnostics/plan_tilt_sweep.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_plan_tilt_full \
  --sample-size 0 \
  --tilts-deg 15
```

上の全件例でも対照と候補15度の2条件を探索する。別seed・全件の実行は自動では行わない。

## 共通にするものと変更するもの

- 予測打点・基準時刻・レバー長・打撃部長・理想の速度ベクトル・攻撃角は共通。
- 保存された角度実行誤差も同じ値を使う。誤差を再抽選したり、接触結果から誤差を選んだりしない。
- 意図したバット軸の傾斜を変え、その傾斜に対応する支点・角速度・並進成分を `plan_swing` で生成する。
- 各参照計画から `SwingPlan` の開始姿勢を生成し、同じsin²速度パルスで前向きに実行する。実際の接触位置への再調整はしない。

理想の打点は共通だが、角度実行誤差を残すため、実際の基準姿勢の位置は条件間で少し変わりうる。`reference_target_error_m` で監査する。理想速度は固定でも、接触時刻・軸上の接触位置が変われば実際の点速度は変わるため、EVだけでなく速度集計も見る。

角度だけを変えて既存の法線を流用すると、軸と法線の直交条件や接触位置が崩れる。そのため、この比較では新しい軌道について接触を探索する。衝突係数・3つの応答モデル・端部/曲線範囲外の扱いは従来どおり。

## 出力

| ファイル | 内容 |
| --- | --- |
| `plan_tilt_summary.csv` | 傾斜候補と保存傾斜対照の接触数、成功数、フェア方向数、EV/VLA、法線、接触時傾斜、点速度等 |
| `plan_tilt_pairs.csv` | 両条件で成功した同じ投球のEV/VLA・フェア方向率 |
| `plan_tilt_transitions.csv` | 接触/未接触、応答適格性、方向の遷移 |
| `plan_tilt_ev_vla.csv` | 各条件の全成功例・フェア方向例のEV/VLAヒストグラム |
| `plan_tilt_kinematics.csv` | 開始支点・パルス全体の支点高さ・基準点速度・目標誤差等の監査 |
| `plan_tilt_sample.csv` | 選択した投球キーと選択ハッシュ |
| `plan_tilt_states.jsonl` | 計画・開始姿勢・接触状態・応答の保存。時計不成立も保持 |
| `plan_tilt_coverage.csv` | 標本数、各条件の適格/未評価理由、実際の探索回数 |
| `plan_tilt_checks.csv` | 予測打点・時刻・速度の共通性、保存計画の再構成、DBとの照合、応答の整合検査 |
| `plan_tilt_metadata.json` | 母集団・標本・条件・探索上限・入力SHA256・投球回転由来・限界 |

既存形式を再利用し、`scenario=tilt_15deg` 等が候補、`orientation=saved_tilt` が元の傾斜の対照、`planned_tilt` が候補を表す。対照は候補ごとに同じ値を繰り返し記載するので合算しない。

summaryの `stratum=planned_tilt` は**元の保存計画の傾斜帯**による投球の層別で、候補角度そのものではない。候補角度はscenarioを参照する。最初は `stratum=overall`、`scope=all_swings`、`model=finite_I0.00015_et0.4` を比較する。

## 判断する指標

- 時計成立例の接触率は `contact_fraction_of_evaluable_trials`。時計不成立を空振りにしない。
- 成功打球内のフェア方向率は `fair_fraction_of_successful`。VLA・EVは各条件の成功集合なので、pairsで共通成功集合も確認する。
- フェア方向の高VLA率はsummaryの `fair_high_vla / fair_contacts`、低EV率は `fair_low_ev / fair_contacts`。`fair_high_ev` と `fair_downward` も同じ分母を使う。従来の `high_vla` / `low_ev` は全成功例の件数なので区別する。セルごとの分布は `plan_tilt_ev_vla.csv` で確認する。
- `plan_tilt_kinematics.csv` はモデルや接触成功で重複させず、時計成立例を分母にする。`pulse_pivot_below_ground` はパルス開始から終了までのどこかで支点zが0未満になる投球数。支点の並進量はパルス積分に比例するため、各座標の範囲は始点・終点から計算する。
- 支点高さは幾何学上の代理値。身体・腕・地面との完全な実行可能性判定ではない。地面より下の条件も黙って除外せず、打球の改善と同時に監査する。

標本は数千投球でも高角度打球は少数になりうるため、数件の増減で角度を決定しない。角度・速度・位置が結び付いた計画仮説の感度であり、観測集計だけで示された傾斜との相関が因果であるとは前提にしない。試合本体の能力値・確率・係数は変更しない。

実行後はsummary、pairs、kinematics、coverage、checks、metadataの6ファイルを共有する。フェア限定EV/VLA分布まで調べる場合はev_vlaも共有する。

## 保存結果の方向・高さ別集計

[HLA・高さ・左右打席別の保存結果診断](plan_tilt_direction_audit.md)では、上のstatesと集計ファイルだけを使って、中央方向への集中と高VLAの関係を確認できる。seed 0/1のフォルダをまとめて指定し、標本ごとに分けて出力する。DB読取り・接触探索・衝突応答の再計算は不要。

## 傾斜と水平向きの分離

[傾斜×yawの小標本診断](plan_tilt_yaw_sweep.md)で、水平向きを独立した計画パラメータとして比較できる。yaw=0は従来と一致し、このコマンドの既定動作は維持する。summaryにはフェアHLAの平均・中央方向割合・高VLA打球のHLA符号別件数も追加された。
