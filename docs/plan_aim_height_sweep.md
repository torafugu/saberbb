# 予測球中心と上下の狙い打点を分ける診断

水平yawの役割を確認した後、低EV不足と下向き打球過多について、予測した球の中心をバット基準点で狙う計画が与える影響を調べる。予測を変更せず、意図した狙い点の上下差を独立した計画入力にする。

## 実行

```sh
python3 tools/diagnostics/plan_aim_height_sweep.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_plan_aim_height_sample \
  --sample-size 1000 \
  --sample-seed 0 \
  --tilts-deg 0 15 \
  --yaws-deg 0 \
  --aim-z-mm -10 0 10
```

元の観測更新出力と、それを生成したDBを使う。DBは既存の `SABERBB_DB` から読み取り専用で参照し、`--db PATH` があれば優先。選手・試合の再生成は不要。出力先は新しいフォルダを指定する。

既定は同じ1,000投球に傾斜0/15度×yaw 0度×上下−10/0/+10 mmの6条件と、保存計画の対照1条件を適用する。探索は最大7,000回。時計不成立は全条件で未評価とし探索しない。対照は投球ごとに一度だけ探索する。実データでの所要時間は未測定。

標本は従来と同じ投球キー・seedのハッシュ順で、接触結果やEV/VLAから選ばない。元データとseedが同じなら前回yaw診断と同じ1,000投球になる。上下オフセットや角度は感度確認用の仮定で、発生頻度や推奨値ではない。

## 計画の分離と符号

`planned_aim_point_world_m = forecast_position_world_m + planned_aim_offset_world_m`

- `forecast_position_world_m` は予測した球の中心。元の予測誤差・予測時刻を維持する。
- `planned_aim_offset_world_m` は意図したバット基準点のずらし量。今回のCLIではworld zだけを指定する。
- 負の値は予測球中心より下、正の値は上をバット基準点で狙う。
- `planned_aim_point_world_m` と `ideal_point_world_m` は意図した狙い位置。角度実行誤差を適用した実際の基準姿勢は `actual_point_world_m`。

狙いを変えた差は基準点・支点・開始姿勢へ反映する。軸、角速度、並進速度、理想速度、基準時刻、保存された角度実行誤差は同じ角度/yawの0 mm条件と共通。各計画の開始姿勢から同じsin²速度パルスで前向きに実行し、実際の球位置や接触結果へ到達位置を補正しない。

実際の球の状態は接触評価にだけ使い、狙いの生成には渡さない。VLA・EVを結果側で補正せず、新しい軌道で接触を探索する。衝突係数・球回転・端部/曲線範囲外の採否は従来どおり。

下側狙いは上向きの法線を作る方向だが、実際のVLAは予測誤差・入射球速度・軸姿勢・摩擦等にも依存する。すべての投球で上向き打球になることを仮定しない。

## 出力と監査

接頭辞は従来と同じ `plan_tilt_`。最初に次の6ファイルを共有する。

- `plan_tilt_summary.csv`
- `plan_tilt_pairs.csv`
- `plan_tilt_kinematics.csv`
- `plan_tilt_coverage.csv`
- `plan_tilt_checks.csv`
- `plan_tilt_metadata.json`

sample、states、transitions、EV/VLA表も保存する。scenarioは `tilt_15deg_yaw_0deg_aim_z_-10mm` 等。orientationの `saved_tilt` は元の保存計画・意図したオフセット0の対照、`planned_tilt` は候補。繰り返された対照を合算しない。stratum=planned_tiltは元の保存計画の傾斜帯。

summaryは従来と同じく、接触率・フェア方向率・EV/VLA・フェア限定の低EV/高EV/下向き/高VLA件数に加え、HLA絶対値平均・打者基準HLA平均・中央±10度割合・高VLAのHLA符号別件数を保持する。

kinematicsには次を追加する。

| 指標 | 意味 |
| --- | --- |
| `avg_planned_aim_z_offset_m` | 意図した上下ずらし量（m） |
| `avg_planned_aim_z_m` | 意図した狙い点の高さ（m） |
| `avg_reference_aim_error_m` | 角度実行誤差適用後の基準点と、意図した狙い点との差 |

既存の `avg_reference_target_error_m` は予測球中心からの差なので、意図したオフセットと実行誤差の両方を含む。今回の意図と誤差の分離では `reference_aim_error_m` も合わせて見る。支点の最低高さ、地面下になる件数、基準点速度・接触点速度も確認する。

metadataはversion=3。scenariosにtilt_deg、yaw_batter_deg、aim_z_offset_mを保持し、aim_z_offsets_mに入力条件を記録する。statesのforecast_planに予測位置と狙い点/オフセットを別々に保存する。既存のSwingPlan.reference_pointは角度誤差適用後の基準点であり、意図した狙い点そのものとは区別する。

0 mmでは以前の傾斜/yaw診断の物理的な軌道・接触結果と一致する。古いコマンドの既定動作は維持し、Rust本体の能力値・計画・確率・係数は変更しない。

## 保存結果の詳細集計

```sh
python3 tools/diagnostics/plan_tilt_direction_audit.py \
  --input-dir ../saberbb_plan_aim_height_sample \
  --output-dir ../saberbb_plan_aim_height_direction_audit
```

DB・接触探索なしで読める。宣言した傾斜/yaw/上下オフセットと保存計画を照合する。高さ別の層別は意図した狙い点ではなく、変更していない予測球中心の高さを使うため、条件間で高さ帯の投球集合を揃えられる。旧metadata version=1/2も引き続き読める。

打球集合は条件によって変わるため、各条件の打球内割合だけでなく、試行当たりの接触・フェア方向率と、pairsの共通成功集合も見る。低EVが増えても空振り・端部除外・支点地面下が増えすぎないか確認する。小標本での感度を調べる段階であり、実測分布への自動フィットや上下狙いの発生頻度は導入しない。
