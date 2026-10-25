# 空中捕球の0.05秒／0.01秒比較

既存DBには `migrations/upgrades/add_fielding_air_probe_comparison.sql` を **1回** 実行する。
前提は `add_fielding_air_probe.sql` 適用済み。新規DBは最新DDLを使用する。
その後、新しい試合を実行する。旧行は新しい11列がNULLのままで、0として扱わない。
以前に抽出した単打120球を遡って再計算する機能ではない。同じ抽出条件に該当する
新しいデータで原因を比較する。試合を再生成しても、過去の条件が揃わなければ同じ120球にはならない。

通常の守備処理・乱数・担当選択は変更しない。全野手に対する診断だけを追加する。
既存 `air_probe_*` は0.01秒、新規 `air_probe_coarse_*` は0.05秒。
どちらも同じ関数、野手能力、打球推定、高さ・時間・要求余裕判定を使用する。
どちらも0.10秒から開始し、最初のバウンド・フェンス接触・打球終了に達する前まで、
成功後も探索する。方向・距離フィルタ、エラー抽選、捕球後の処理は適用しない。
0.01秒の整数tickのうち5点ごとを0.05秒版が使用するため、共通時刻での入力は一致する。
通常探索の浮動小数点加算で生成される時刻をそのまま再現するものではない。
比較する2つの診断の違いは探索間隔のみで、対象区間の境界が一致しても最終サンプル時刻は異なり得る。

新規列は既存の11列と同じ意味を持つ（接頭辞だけ `air_probe_coarse_`）。
段階別件数、最大余裕・時刻・高さ、最初の成立時刻・高さ・余裕を保存する。
全空中区間の探索なので件数自体は通常探索の件数と比較しない。
0.05秒版を追加する分の計算コストが増える。各診断内では軌道サンプルを全野手で共有する。

## 同じ野手での比較一覧

以前の `normal_failed_but_probe_possible` と同じ条件、すなわち外野手で
`evaluated = 1 AND risk_ok_samples = 0 AND air_probe_risk_ok_samples > 0` の単打を抽出する。
1行は1球×1野手。複数野手が該当すると同じ球が複数行になる。
`sampling_gap` は0.01秒版だけ成立、`coarse_also_possible` は両方成立。
後者は通常探索との終了条件・エラー・時刻の丸めなどを確認する対象であり、
この分類だけではその原因を特定できない。捕球可能判定は実際のアウトを保証しない。

```sql
SELECT c.game_id, c.count_seq, c.fielder_id, c.fielder_position,
       c.selected, c.selected_path, c.end_reason,
       c.interception_catch_type, c.interception_error_type,
       c.last_evaluated_time_sec,
       c.air_probe_coarse_risk_ok_samples AS coarse_ok_samples,
       c.air_probe_risk_ok_samples AS fine_ok_samples,
       c.air_probe_coarse_first_catch_time_sec AS coarse_first_time_sec,
       c.air_probe_first_catch_time_sec AS fine_first_time_sec,
       c.air_probe_coarse_max_margin_sec AS coarse_max_margin_sec,
       c.air_probe_max_margin_sec AS fine_max_margin_sec,
       c.air_probe_coarse_max_margin_time_sec AS coarse_max_time_sec,
       c.air_probe_max_margin_time_sec AS fine_max_time_sec,
       c.air_probe_coarse_max_margin_height_m AS coarse_max_height_m,
       c.air_probe_max_margin_height_m AS fine_max_height_m,
       c.required_margin_sec, b.first_bounce_time, b.fence_impact_time,
       CASE WHEN c.air_probe_coarse_risk_ok_samples = 0 THEN 'sampling_gap'
            ELSE 'coarse_also_possible' END AS diagnosis
FROM player_game_fielding_candidate c
JOIN player_game_batting b
  ON b.game_id = c.game_id AND b.count_seq = c.count_seq
WHERE b.result = 'Single'
  AND c.fielder_position IN ('LF', 'CF', 'RF')
  AND c.evaluated = 1 AND c.risk_ok_samples = 0
  AND c.air_probe_risk_ok_samples > 0
  AND c.air_probe_step_sec = 0.01
  AND c.air_probe_coarse_step_sec = 0.05
  AND c.air_probe_coarse_risk_ok_samples IS NOT NULL
ORDER BY c.game_id, c.count_seq, c.fielder_position;
```

## 打球単位の重複なし集計

対象野手のうち1人でも0.05秒で成立すれば `coarse_also_possible`、
全員0.05秒では不成立なら `sampling_gap_only`。両分類は排他的。
対象野手の比較記録が一部欠ける球は除外し、未記録を不成立と混同しない。
表示する打球数の合計が今回の比較対象数になる。

```sql
WITH targets AS (
    SELECT c.*
    FROM player_game_fielding_candidate c
    JOIN player_game_batting b
      ON b.game_id = c.game_id AND b.count_seq = c.count_seq
    WHERE b.result = 'Single'
      AND c.fielder_position IN ('LF', 'CF', 'RF')
      AND c.evaluated = 1 AND c.risk_ok_samples = 0
      AND c.air_probe_risk_ok_samples > 0
), per_ball AS (
    SELECT game_id, count_seq, COUNT(*) AS target_fielders,
           MAX(CASE WHEN air_probe_coarse_risk_ok_samples > 0 THEN 1 ELSE 0 END) AS coarse_possible
    FROM targets
    GROUP BY game_id, count_seq
    HAVING MIN(CASE WHEN air_probe_step_sec = 0.01
                         AND air_probe_coarse_step_sec = 0.05
                         AND air_probe_coarse_risk_ok_samples IS NOT NULL
                    THEN 1 ELSE 0 END) = 1
), classified AS (
    SELECT *, CASE WHEN coarse_possible = 1 THEN 'coarse_also_possible'
                   ELSE 'sampling_gap_only' END AS diagnosis
    FROM per_ball
)
SELECT diagnosis, COUNT(*) AS balls, SUM(target_fielders) AS target_fielder_rows
FROM classified
GROUP BY diagnosis
ORDER BY diagnosis;
```

最初に打球単位の内訳を確認し、次に一覧で各球の成立区間と要求余裕を見る。
`sampling_gap_only` が多ければ細かい刻みの効果を確認できる。
`coarse_also_possible` が多ければ通常探索との差の調査を続ける。
本変更は実際の捕球判定やフィルタの変更には進めない。
