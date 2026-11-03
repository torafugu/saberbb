# 保存済み実行状態の混合とStatcast比較

計画effort=1、実行比率1/0.95/0.90/0.85の保存結果を、接触探索やDB読取りをせずに混合する。通常実行と各失敗状態の単独分布を対照として常に出力し、JSONで指定した混合分布を追加する。発生確率の自動推定や本体への導入は行わない。

## 実行

前回の `swing_execution_probe.py` の出力フォルダを指定する。以下の `../saberbb_execution_ratio_sweep` は、実際に保存したフォルダ名に合わせる。

```sh
python3 tools/diagnostics/swing_execution_mixture_compare.py \
  --input-dir ../saberbb_execution_ratio_sweep \
  --statcast-csv ../saberbb_finite_statcast_input/savant_data.csv \
  --mixtures-json tools/diagnostics/execution_mixtures.example.json \
  --output-dir ../saberbb_execution_mixture_comparison
```

`SABERBB_DB` は不要。選手・試合の再生成も不要。標準ライブラリのみで動作し、保存済み集計表とStatcast CSVを読む。接触探索の実行時間はかからないが、CSV読取り時間は入力件数に依存する。出力先は新しいフォルダを指定する。

付属JSONの失敗率10%/25%は**感度比較の例**で、推奨値・実測値・ゲームの既定値ではない。失敗時の0.95/0.90/0.85の配分も50%/30%/20%という任意の仮定。必要に応じてコピーして編集する。`--mixtures-json` を省略すると、各状態の単独分布のみ比較する。

```json
{
  "version": 1,
  "mixtures": {
    "my_assumption": {
      "plan_1_execution_1_anchor_0.5_delay_0ms": 0.9,
      "plan_1_execution_0.95_anchor_0.5_delay_0ms": 0.1
    }
  }
}
```

重みは試行段階での状態の発生割合で、0以上、合計1。未指定の状態は重み0。混合名を複数記載して並列に評価できる。状態名は入力metadataの `scenarios` に一致させる。同じ混合内では計画effort、anchor_fraction、start_offset_sec、試行数・評価可能数を揃える。開始遅れが違う条件は時計による除外集合も異なりうるため、集計表だけでは混ぜず、エラーにする。

## 必要な入力

通常は同じ実行から得た次の3ファイル。

- `swing_execution_metadata.json`（version=2）
- `swing_execution_summary.csv`
- `swing_execution_ev_vla.csv`

通常ヒストグラムがなければ `swing_execution_deviation_ev_vla.csv` を読む。`--histogram-csv PATH` で明示指定も可能。deviationの `executed` を各実行状態、`planned` を対応する比率1・遅れ0の計画対照として扱う。複数条件に繰り返し記録されたplannedは一致を検査して一度だけ使う。summaryは通常の `swing_execution_summary.csv` を使う。古いeffortだけの診断や異なる実行のファイルを混在させない。

対象scopeは既定 `all_swings`。`--scope recorded_batting_only` に切り替えられるが、実データと厳密に同じ打球集合になる保証はない。variant/modelも指定可能で、既定は `observed_sigma_0.005_r0.9` / `finite_I0.00015_et0.4`。stratum=overallだけ使い、傾斜などの重複集計を加算しない。

## 選択後の混合

状態sの試行重みをw、時計が成立する試行数をN、特定セルの打球数をc、そのcohortの全打球数をFとする。

- セルの試行当たり確率: `sum(w_s * c_s / N_s)`
- cohortが発生する確率: `sum(w_s * F_s / N_s)`
- cohort内のセル割合: 上のセル確率をcohort発生確率で割る

これにより、接触率やフェア方向率が違う状態を適切に条件付ける。正規化済みの打球割合を試行重みでそのまま平均しない。時計不成立は未評価としてNから除外し、空振りには数えない。実行比率以外の計画・時計条件を揃えることで、混合対象の評価可能な投球集合を揃える。

同じ重みを全投球に適用する独立割当の期待分布であり、特定の投球・選手・状況で失敗しやすいという相関は表現しない。実際の接触結果を見て状態を割り当てたり、打球EVを後から倍率補正したりしない。重み付きセル値は期待確率で、実際の標本件数ではない。

## 出力と読み方

| ファイル | 内容 |
| --- | --- |
| `execution_mixture_summary.csv` | 単独状態・混合ごとの接触確率、cohort発生確率、低EV・高EV・VLA領域割合、Statcastとの差のTV |
| `execution_mixture_components.csv` | 試行重み `trial_weight` と、選択後の構成比 `selected_cohort_weight` |
| `execution_mixture_ev_vla.csv` | EV/VLAセルの試行当たり確率、cohort内割合、Statcast割合・差（百分率ポイント） |
| `statcast_coverage.csv` | インプレー選択、欠損除外、バント未分類などの監査 |
| `execution_mixture_metadata.json` | 入力SHA256、仮定、指定重み、元の実行metadata、限界 |

`cohort=fair_direction` の分布をStatcastの `type=X` かつEV/VLA両方ありと比較する。`all_successful` はファウル方向等を含む別診断で、StatcastとのTVは出さない。打球0のcohortは条件付き割合・TVを空欄にし、発生確率0として残す。実行状態と混合は同じ投球を共有するため件数を足さない。

EVはm/s（Statcastのmphを0.44704倍）、VLAは度。ヒストグラムの境界は保存済み診断に合わせ、EV=25は低EV側に含めず、EV=45以上を高EV、VLA=50以上を高角度とする。上下の裾も保持する。以前のfinite-barrel比較より粗いビンを使うため、TVの数値を直接比較しない。

summaryの `statcast_*_fraction` と対応するsimulation割合を同時に見る。TVは0に近いほどこのビン分割で分布が近いという記述量であり、最小の混合が実際の失敗率を表すとは限らない。低EV増加と同時に高EVが消えすぎないか、VLAが偏らないかも確認する。

フェア方向は飛翔・地面反射まで含めた最終フェア判定ではない。端部・曲線範囲外等はEV/VLAを持たず、モデル適格性の影響が残る。Statcastの欠損は除外し0で埋めない。推定値が含まれうる点、リーグ・期間・選手構成の不一致、シミュレーションのバント未分類も残る。既存比較と同じ `--bunt-labels` によりStatcast側だけ明示的なバント除外は可能だが、厳密なcohort一致は主張しない。

実行後はsummary、components、coverage、metadataを共有する。セルごとのズレを調べる場合はev_vlaも共有する。
