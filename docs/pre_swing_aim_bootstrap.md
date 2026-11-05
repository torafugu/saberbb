# 固定した打者戦略の投球単位対応ブートストラップ

seed2を見て固定した`line_drive_low_center`（予測高0.6 m未満は0 mm、それ以上は−15 mm）を、
中心狙い`contact_center`と比較する。seed3では通常速度のライナー帯発生率差が約+3.02ポイントだったため、
狙い量・高さ境界・係数を変えず、その差と接触・フェア方向率の不確実性を評価する。

## 実行

```sh
python3 tools/diagnostics/pre_swing_aim_bootstrap.py \
  --input-dir ../saberbb_aim_policy_holdout_seed3 \
  --output-dir ../saberbb_aim_policy_bootstrap_seed3 \
  --policies-json tools/diagnostics/pre_swing_aim_seed2_frozen.json \
  --reference contact_center --candidate line_drive_low_center \
  --replicates 2000 --bootstrap-seed 0
```

入力は**元の接触計算キャッシュ**で、`plan_tilt_states.jsonl`、metadata、sample、summary、EV/VLA、kinematicsを含むフォルダ。
直前の`pre_swing_aim_summary.csv`等の集計結果だけでは、同一投球での共分散を復元できない。
元キャッシュがあればDB・Statcast CSV・接触探索・衝突再計算・新たな試合生成は不要。
Python標準ライブラリのみ。出力先は未作成のフォルダを指定する。

## 再抽出の単位と点推定の監査

1. キャッシュの全投球・全条件、予測・計画・保存結果と元集計の整合性を既存の監査で確認する。
2. 元の診断と同じ予測高だけの選択器で、固定した両戦略の狙いを決める。
3. `game_id/count_seq`を並べ、1投球を1単位として元の標本数と同じ回数、復元抽出する。
4. **同じ抽出回数を両戦略・全速度・全傾斜・全yawに適用する。** 戦略別・速度別に投球を引き直さない。
5. 各回で評価可能投球数、各戦略のフェア方向打球数、共通フェア数を計算し直して指標を求める。

両戦略で空振りの投球や時計不成立の投球も抽出対象に残す。時計不成立は抽出後の評価分母に入れない。
未収束・探索開始時近接は元の診断と同じ扱いを維持し、確定した空振りとは解釈しない。
フェア方向打球だけを抽出する方法や、元のフェア件数を各回の固定分母にする方法は使わない。

同じ全条件の集計ベクトルを持つ投球は内部でまとめるが、各投球の抽出確率と条件間の対応はそのまま保持する。
ブートストラップ用の乱数はローカルな`random.Random`のみを使う。キー順・設定・seed・反復数が同じなら再現する。
再抽出前の点推定が、既存診断のsummary/pairsと一致することをchecksで検証する。

## 出力

最初に次の3ファイルを共有する。

- `pre_swing_aim_bootstrap_summary.csv`
- `pre_swing_aim_bootstrap_checks.csv`
- `pre_swing_aim_bootstrap_metadata.json`

詳細な再抽出値は`pre_swing_aim_bootstrap_replicates.csv`に保存する。通常は共有不要。

summaryは傾斜・yaw・速度・指標ごとに1行。`reference_policy`が中心狙い、`candidate_policy`が新戦略。
差は常にcandidate−reference。`role=primary`は傾斜15度・yaw 0度・速度比1.0の
`line_window_probability_delta_pp`だけで、他は補助指標とする。対象条件がない場合はmetadataの`primary.present=false`になる。

| 指標 | 意味 |
| --- | --- |
| reference/candidate_*_probability_per_evaluable_trial | 各戦略の幾何接触・力積正の接触・フェア方向・下向き・高角度・ライナー帯の件数÷各回の評価可能投球数 |
| *_probability_delta_pp | 上記確率の戦略差。単位はパーセントポイント |
| reference/candidate_*_fraction_among_fair | 下向き・高角度・ライナー帯の件数÷各戦略・各回のフェア方向打球数 |
| *_fraction_delta_pp | 条件付き割合の戦略差。対象打球の入れ替わりを含む |
| common_fair_*_delta_pp | 両戦略でフェア方向となった共通投球内での下向き・高角度・ライナー帯の割合差 |

ライナー帯は10≤VLA<30度、下向きはVLA<0度、高角度はVLA≥50度で、いずれもフェア方向に限る。
`point_estimate`は元標本の値。`percentile_2_5`と`percentile_97_5`は再抽出分布の95%パーセンタイル区間、
`median`は再抽出値の中央値。割合の`units=fraction`は0〜1の値で、差の`units=percentage_points`と区別する。

評価可能投球、各戦略のフェア方向打球、または共通フェアがゼロなら、対応する指標は未定義にする。
`valid_replicates`はその指標を定義できた回数、`undefined_replicates`は未定義だった回数。
未定義を0で埋めず、有効な再抽出値だけで分位点を計算する。有効回数がゼロなら区間も空欄。

## 解釈と限界

主指標の差と区間を読み、接触率・フェア方向率・高角度率の差と区間も併記する。
条件付き高角度率の低下だけで改善と判断しない。共通フェアの指標も全投球への効果とは分けて読む。
反復数は元投球数を増やさず、4速度の結果も独立した4標本にはならない。

これは、固定した戦略・物理モデル・標本を前提とする投球単位のパーセンタイル区間。
同じ選手・試合に属する投球の依存、有限母集団補正、仮説選択の不確実性、観測モデルや係数の不確実性は含まない。
実際のライナー分類や最終フェア判定を検証したものでもない。補助指標の区間は同時信頼区間ではない。
新しい許容幅・合否判定・構成比調整・最良戦略の探索は追加しない。

## テスト

```sh
python3 -m unittest discover -s tests/diagnostics -p 'test_pre_swing_aim_bootstrap.py'
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
```

同じ投球を共有する戦略・速度間の共分散、同じ戦略同士の差ゼロ、圧縮前後の抽出確率、
可変分母・空集団・時計不成立、既存診断との点推定一致、比較を逆にしたときの差と区間の反転、
seedによる再現性、設定ハッシュ保存、DB・探索・衝突計算の不使用を確認する。
