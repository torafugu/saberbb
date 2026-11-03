# StatcastとのEV×VLA比較

目的は低EVの裾・下向きの角度分布の違いを確認すること。
衝突モデルや係数は変更せず、実測への校正完了とは扱わない。

## 比較集合

シミュレーションは `finite_barrel_spin_ev_vla.csv` の1 scope・variant・modelと
入力回転側の度数を使用する。新しい既定 `input_fair_common` は当該モデルの
入力回転側がフェア方向となる共通成功集合で、ゼロ回転側の方向は問わない。
従来の `reference_fair_common` はモデル間の固定集合比較には有用だが、
実測とのフェア打球比較では別の選別が入るため既定にしない。

ただし、方向はフェア打球の最終判定ではない。ファウル側へ落ちる球や
その後の地面挙動などは分類していない。バット端面・文献曲線範囲外などの
未評価接触も含まれず、元の全BBEを表すとは限らない。
ヒストグラムにはバント情報がない。各モデルの分母はモデル別であり、
モデル間で同一集合になる保証はない。

Statcast側は `type=X` のインプレー球からEV・VLAが両方ある行を選ぶ。
ホームラン・アウト・安打等の結果で選別しない。必要列は
`game_pk,at_bat_number,pitch_number,type,launch_speed,launch_angle`。
同一投球キーの重複は停止する。`launch_speed` はmphからm/sへ0.44704倍で変換。
欠損はゼロ補完せず、EV欠損・VLA欠損・いずれか欠損・採用数を別記録する。
その割合は選択したインプレー集合に対するもの。欠損EVと欠損VLAは重複する。
負のEVや±90度を超えるVLA、無限大、不正な数値文字列は停止する。

公式仕様: https://baseballsavant.mlb.com/csv-docs
EV・VLA列には推定値が含まれる場合がある。完全な値があることだけでは
直接計測されたと断定できない。CSVのdescriptionや犠打結果だけから、
全成功バントを漏れなく識別できるとは仮定しない。

## 実測CSVの準備

Baseball SavantのStatcast Searchから、対象期間・リーグを固定したCSVを保存する。
最初は完了した2025年MLBレギュラーシーズンなど、範囲が明確なデータを推奨。
EVやVLAで検索を絞ると裾が失われるため、それらを取得条件にしない。
分割取得ならCSVを1つへ統合し、重複期間・投球キーがないことを確認する。
この診断はオンライン取得を実施せず、指定したCSV全体を読む。
期間・リーグ・推定値の採否など、検索条件は元データと一緒に保存する。

バントを除外する場合、各インプレー球について次の列を持つ注釈CSVを用意する。
`game_pk,at_bat_number,pitch_number,is_bunt`。is_buntは0または1であり、
全インプレー球に判定が必要。キー重複・判定欠落は停止する。
判定根拠を確認できない打球を0と置かない。

`--bunt-labels`を省略するとバントを含み得る探索比較として動作し、
metadataへ `bunt_policy=unknown_included` を記録する。
この結果をバント除外済み比較とは呼ばない。
明示的注釈があっても、シミュレーション側の選別差は残る。

## 実行

最新の有限半径診断で `input_fair_common` を出力するため、同じ観測更新CSVと元DBから
有限半径診断だけ再実行する。選手・試合・観測更新CSVの再生成は不要。

```sh
python3 tools/diagnostics/finite_barrel_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_finite_statcast_input \
  --pitch-db "/path/to/saberbb.db" \
  --gyro-sign 0

python3 tools/diagnostics/statcast_distribution_compare.py \
  --simulation-csv ../saberbb_finite_statcast_input/finite_barrel_spin_ev_vla.csv \
  --statcast-csv "/path/to/statcast_2025_regular.csv" \
  --output-dir ../saberbb_statcast_comparison
```

バントの全件注釈がある場合は `--bunt-labels /path/to/bunt_labels.csv` を追加。
既存出力先の上書きは拒否する。入力ファイルは変更しない。
`--model`、`--variant`、`--scope`、`--cohort`で集合を明示的に変更できる。
既存CSVでは `--cohort reference_fair_common` を指定した探索比較も可能だが、
実測側と集合が一致した比較とは解釈しない。

## 出力と評価

- `statcast_coverage.csv`: 取得行数から採用行数までの段階別件数と欠損。
- `statcast_ev_vla_comparison.csv`: 同じ区間の2次元度数・割合・差（simulation−Statcast、百分率ポイント）。
- `statcast_regions.csv`: EV<25m/s、VLA<0度、10≤VLA<30度の領域比較。
- `statcast_comparison_metadata.json`: 入力SHA256、採用集合、分母、欠損を除いた採用率、
  単位変換、粗い区間上の全変動距離、比較上の制約。

区間は従来どおりEVが5m/s、VLAが10度刻み。下限を含み上限を含まない。
両端の裾も保存し、欠損とは区別する。全セルを出力するため、ゼロ度数セルもある。
実測とシミュレーションの標本数を等しく間引かず、それぞれの採用数で正規化する。
全変動距離はこの粗い格子での差であって、有意差検定や個別打球の移動率ではない。
VLA<0は下向き打ち出しの指標で、接地後のゴロの弱さを示さない。

まず欠損・バント判定・モデル適用範囲を確認し、そのうえで低EVの裾と角度分布を読む。
このツールは実測データを同梱せず、未取得時は比較結果を生成しない。
