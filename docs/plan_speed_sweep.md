# 意図したスイング速度の比較

通常速度の40/60/80/100%で振る計画を同じ標本に適用し、低EV不足に速度の選択肢が関係しているかを調べる。

```sh
python3 tools/diagnostics/plan_speed_sweep.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_plan_speed_sample \
  --sample-size 1000 \
  --sample-seed 0 \
  --tilts-deg 0 15 \
  --speed-ratios 0.4 0.6 0.8 1
```

元データに対応するDBを環境変数 `SABERBB_DB` から読み取り専用で取得する。`--db PATH` があれば優先。選手・試合の再生成は不要。出力先は新しいフォルダを指定する。

既定は傾斜0/15度×計画速度4条件の8条件と、元の保存計画の対照1条件。同じ1,000投球を接触結果によらず選び、最大9,000探索する。時計不成立は未評価で、空振りへ含めない。傾斜・yaw・狙い位置・速度を増やすと直積で探索数が増える。yawと上下狙いは既定0で、`--yaws-deg` と `--aim-z-mm` で変更できる。

## 計画速度と実行倍率

予測した球中心・時刻、狙い位置、バット軸、保存された角度実行誤差を維持し、計画の理想速度・実際の基準速度・支点速度・理想/実際の角速度を指定比率で縮める。その後、各計画のsin²速度パルスを逆算して開始姿勢を作る。開始後は同じパルスで前向きに実行し、新しく接触を探索する。実際の球位置や接触結果で到達位置を補正しない。

`planned_speed_ratio` は元の計画速度に対する比率。`plan_effort=1` と `execution_ratio=1` は再計画後の運動に対する倍率である。`planned_speed_ratio=0.4` は、元の全速計画を40%しか実行できなかった条件とは異なる。スイング開始の時刻・パルス時間・基準時刻の位相は全条件で共通。

100%条件は同じ傾斜/yaw/狙いの従来診断と軌道・接触・衝突応答が一致する。傾斜0/15度の100%と、元の保存傾斜を使うsaved_tilt対照は別条件。遅く振ることで精度が向上する仮定は加えず、角度実行誤差は共通にする。

比率の許容範囲は0より大きく1以下。これは低速の計画を調べる感度設定で、実際の選手の選択頻度ではない。ハーフスイングの途中制動、方向と速度の結び付き、構えからの人体運動の成立条件は今回の範囲に含めない。各速度で開始姿勢が異なるため、固定した構えから全条件を実行できると主張しない。

## 出力と共有

従来と同じ `plan_tilt_` 接頭辞を使う。最初に共有するのは次の6ファイル。

- `plan_tilt_summary.csv`
- `plan_tilt_pairs.csv`
- `plan_tilt_kinematics.csv`
- `plan_tilt_coverage.csv`
- `plan_tilt_checks.csv`
- `plan_tilt_metadata.json`

metadata version=4。scenariosにplanned_speed_ratio、最上位にplanned_speed_ratiosを追加。scenarioは `tilt_15deg_yaw_0deg_aim_z_0mm_speed_r0.4` 等。kinematicsにavg_planned_speed_ratioを追加し、実際の速度はavg_reference_point_speed_msと接触点速度で確認する。低EVは25 m/s未満、高EVは45 m/s以上。

繰り返し記載されたsaved_tilt対照を合算しない。打球内の低EV率だけでなく、試行当たりの接触・成功・フェア方向の件数、時計不成立、端部/曲線範囲外、支点地面下も合わせて見る。pairsは元の保存計画との共通成功集合であり、低速対100%を直接揃えたペアではない。

## 保存結果の詳細診断

```sh
python3 tools/diagnostics/plan_contact_ev_audit.py \
  --input-dir ../saberbb_plan_speed_sample \
  --output-dir ../saberbb_plan_speed_contact_ev_audit
```

こちらはDB・接触探索・衝突再計算なし。速度比を宣言したmetadataと保存計画の一致、比率を戻した理想速度が元計画と共通であることを検証する。従来のdirection auditもversion=4を読める。旧version=1/2/3の読み取りと、既存の診断コマンドの既定動作は維持する。

小標本の条件比較であり、Statcastへの自動フィット、速度条件の確率的混合、Rust本体や能力値・衝突係数の変更は行わない。
