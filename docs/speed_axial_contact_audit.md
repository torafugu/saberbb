# 実接触の速度・軸方向打点と低EVの照合

固定条件の `impact_parameter_sweep.py` で、バット点速度35m/sでは調べた15ケース全点がEV>=25m/s、速度30m/s・先端側打点+0.10mでは弱いフェア方向打球が出ることを確認した。次に、保存済み接触がどの速度・軸方向位置に分布し、その条件で実際にどのEVを出しているかを調べる。

```sh
python3 tools/diagnostics/finite_barrel_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_speed_axial \
  --gyro-sign 0 \
  --diagnostics speed_axial \
  --models finite_I0.00015_et0.4 \
  --variants observed_sigma_0.005_r0.9
```

DBパスは[共通設定](diagnostic_database.md)の `SABERBB_DB` から取得。先に `export SABERBB_DB="/実際の保存先/saberbb.db"` を一度設定する。明示した `--pitch-db` が優先。回転の比較条件を前回と揃えるため、同じDBとgyro-signを使う。SABERBB_DB未設定でDB引数もない場合は従来どおり固定ゼロ回転なので、実データの回転を使う比較では設定を確認する。

入力は観測更新診断のCSV・メタデータ・coverage。選手や試合の再生成、接触探索は不要。応答計算は対象variant・主モデルと法線対照2種類に限定し、接地後応答とゼロ回転対照を省略する。全入力CSVの読取り・ハッシュ・保存値再構成・キー完全性監査は維持する。所要時間は実ユーザーデータで未測定。

`speed_axial` は明示指定したときだけ出力する。`--diagnostics` 未指定の既存出力・既定集計範囲は維持し、新しいクロス集計の負荷を毎回追加しない。`--diagnostics tilt speed_axial` のように他の集計と併用可能。

## 座標・区分

- 速度は接触時のバット中心線上の接触点の世界速度の大きさ。表面速度、重心速度、スイング開始時の設定速度と区別する。
- 軸方向打点は `closest_axial_at_entry_m`。保存基準点を0、軸の正方向（先端側）を正とする。先端側+0.10mはバット先端から10cmという意味ではない。
- `source_z_m = 0.71m + closest_axial_m` は位置別反発曲線への対応座標で、地面からの高さではない。
- signed b/Rは接触時の凍結局所相対中心速度を基準に定義。前回の局所b掃引と同じ符号の定義を使うが、保存接触では距離許容誤差による微小補正を含む。
- 軸傾きは保存された有向軸の符号付き仰角。正が+Z方向、負が-Z方向。軸の向きに依存し、軸全体の符号反転に不変なunsigned tiltとは異なる。速度角は接触点速度の水平成分に対する仰角。
- 端部は `contact_region=cap` として保持し、側面の軸方向打点分布と混ぜない。端部のb/Rはundefined。

速度境界は20/25/30/35/40/45m/s、軸方向境界は-0.12/-0.08/-0.04/0/0.04/0.08/0.12m。区間は下端を含み上端を含まない。たとえば先端側+0.10mは `0.08..<0.12`、速度30m/sは `30..<35`。範囲外の値も外側の区間に保持する。

## 出力

| ファイル | 内容 |
| --- | --- |
| `finite_barrel_speed_axial_geometry.csv` | モデルに重複させない実接触分布、平均・最小・最大、幾何採用可否 |
| `finite_barrel_speed_axial_outcomes.csv` | 保存元応答/各モデルの状態・方向別EV/VLA、最低EV、低EV等の件数 |
| `finite_barrel_speed_axial_coverage.csv` | 非接触・未解決・未評価も含む全対象試行の件数 |
| `finite_barrel_speed_axial_checks.csv` | 保存点速度とベクトル長、反発曲線座標の一致検査 |

併せて通常の `finite_barrel_checks.csv`、coverage、metadataとDB回転使用時の不正回転レポートを出力。metadata version=10に本診断の解釈と境界を記録する。

各ファイルのstratum:

- `overall`: 全接触の検算用。
- `speed_axial`: 速度×軸方向打点。
- `speed_axial_impact`: 上記に符号付きb/R区間を追加。
- `speed_axial_tilt`: 速度×軸方向打点に符号付き軸傾き区間を追加。

stratumやscope、応答モデルは重なるため合算しない。geometryは全幾何接触（端部/曲線範囲外を含む）が分母で、モデル数では増えない。非接触試行に速度や打点を補ってgeometryへ含めず、coverageに残す。

outcomesは同じ幾何接触を保存元と各モデルに対して1回ずつ計数する。`collision_status=impulse_contact` の行にだけEV/VLAを記録。未評価では `n_ev_ms=0`、平均/最小/最大は空欄。EV<25m/sは `ev_lt25_contacts`、フェア方向は `direction=fair_direction`。各 `n_` が対応する平均と割合の分母。

## 確認する順序

1. coverageと通常のchecksで入力と応答の整合を確認する。
2. geometryを `scope=all_swings`、対象variant、`contact_region=side`、`stratum=speed_axial` に限定し、モデル採用前の速度/打点分布を見る。`geometry_reason=eligible` に絞った場合との差も確認する。
3. outcomesを同じscope/variant/側面/stratum、`outcome_basis=finite_I0.00015_et0.4`、`collision_status=impulse_contact` に限定する。同じ速度・打点の全方向とフェア方向を比較し、低EV条件が方向選別で消えていないかを見る。
4. 速度25〜30m/s付近と先端側0.08〜0.12m付近に、接触がどれだけあり、低EVが出ているかを確認。必要に応じてb/Rと傾きのクロス集計、平均/最小/最大の投球速度と速度角も使う。

掃引の固定条件（投球38m/s、速度角5度、仮定した回転など）と実データには差がある。近い速度/打点区間に入るだけで弱い打球が出るとは判定せず、掃引の最低EVを実接触へ代入しない。この集計は実際の接触分布と応答を示し、固定条件の結果と照合するためのもの。必要なら次段階で実接触の各入力を固定した局所掃引を行う。

今回の結果はユーザーのCSV/DBに依存するため、実行後に上記4ファイルと `finite_barrel_checks.csv` を共有して確認する。
