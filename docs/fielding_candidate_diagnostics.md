# 全野手の空中捕球候補診断

## 導入と記録範囲

既存DBでは `migrations/upgrades/add_fielding_candidate_diagnostics.sql` を実行する。
この移行は既存テーブルを変更せず、繰り返し実行できる。
新規DBでは `migrations/ddl/player_game_fielding_candidate_table.sql` を使用する。

移行後に新しく実行した試合について、`process_fielding()` に渡された全野手を
`player_game_fielding_candidate` に保存する。主キーは
`(game_id, count_seq, fielder_position)`。通常は守備処理した1球につき9行となる。
過去の試合や、見送り・空振り・本塁打などで守備処理に入らない球には記録しない。
野手数が9人未満なら実際に渡された野手数となる。

候補選別・捕球計算・探索刻み・野手の評価順・乱数消費は従来と同じ。
候補から外れた野手を診断のために再評価することはない。
捕球・ファウル・走塁処理の早期終了より前に診断を保持し、試合結果と同じトランザクションで保存する。
そのため、後続処理で打撃記録が作られない特殊な経路では、打撃記録なしで候補診断が存在し得る。

## 列

| 列 | 内容 |
|---|---|
| `game_id`, `count_seq` | 試合・投球キー |
| `fielder_position`, `fielder_id` | この候補の野手位置・ID。送球先とは独立 |
| `lane_eligible` | 方向による候補条件を満たすか（0/1） |
| `distance_eligible` | 現行の `fielder.distance() <= ball.distance()` を満たすか（0/1） |
| `evaluated` | 両条件を満たし、実際に捕球探索を実行したか（0/1） |
| `air_samples` | 探索中に軌道推定が `is_direct = true` とした点数 |
| `height_ok_samples` | 上記のうち捕球可能な高さだった点数 |
| `reachable_samples` | さらに野手所要時間が打球到達時刻以下だった点数 |
| `risk_ok_samples` | さらにリスク設定の要求余裕を満たした点数。エラー判定前 |
| `max_air_margin_sec` | 捕球可能な高さの空中探索点での最大 `t - fielder_needed_time`（秒）。負値も保存 |
| `max_air_margin_time_sec` | 最大余裕が最初に観測された打球発生後の時刻（秒） |
| `max_air_margin_height_m` | その探索点の推定打球高さ（m） |
| `required_margin_sec` | 野手のリスク設定が要求する余裕。0 / 0.15 / 0.35秒 |
| `last_evaluated_time_sec` | 実際に最後に評価した探索時刻（秒） |
| `end_reason` | 候補除外・探索終了理由（下表） |
| `interception_catch_type` | この野手の時刻別探索が返した捕球種別。成立なしなら `NULL` |
| `interception_error_type` | その探索が返したエラー種別。なしなら `NULL` |
| `interception_time_sec` | その探索が返した捕球・処理時刻。成立なしなら `NULL` |
| `selected` | カバー・最終拾球まで含め、実効的な担当に選ばれたか（0/1） |
| `selected_path` | 選ばれた場合の `interception` / `covering` / `final_pickup` |

| `end_reason` | 意味 |
|---|---|
| `filtered_lane` | 方向条件で除外。距離条件も不成立の場合はこちらを優先 |
| `filtered_distance` | 方向条件は成立したが距離条件で除外 |
| `direct_fly` | 時刻別探索で正常な空中捕球が成立 |
| `bounce_catch` | 時刻別探索で正常な着地後捕球が成立 |
| `fielding_error` | 時刻別探索がファンブル・逸球を返した |
| `no_interception` | 探索を終了したが成立地点なし |

### NULL・0・最大値の読み方

- `evaluated = 0`：探索件数・最大余裕・探索時刻は `NULL`。
- `evaluated = 1 AND air_samples = 0`：実際に探索したが、空中扱いの地点は0件。
- `height_ok_samples = 0`：最大余裕の対象地点がないため、最大余裕関連3列は `NULL`。
- `max_air_margin_sec < 0`：評価した捕球可能高さの空中地点すべてで時間不足。
- `reachable_samples > 0 AND risk_ok_samples = 0`：時間内に届く地点はあったが余裕条件を満たさない。
- `risk_ok_samples > 0`：空中捕球の幾何・時間・リスク条件は通過。捕球エラーの有無は別に確認する。

**件数と最大余裕は、既存処理が実際に探索した範囲の値**である。
最初に捕球地点が成立したら従来どおり終了し、その後を追加探索しない。
したがって将来の軌道全体での理論的な最大余裕ではない。
現在の実装では空中のリスク条件を満たすと捕球・エラー結果を返すため、`risk_ok_samples` は最大1。

`air_samples` は物理的な正しさを別途保証する列ではなく、現行の軌道推定の空中判定を観測した列。
`max_air_margin_time_sec` を打撃記録の着地・フェンス衝突時刻と照合し、軌道推定の整合性を確認する。

`selected = 1` でも `evaluated = 0` の場合がある。
候補選別では除外されたが、後続の `final_pickup` や `covering` で選ばれたケースである。
`interception_*` は各野手の最初の時刻別探索の結果であり、選択後の拾球結果とは異なる。
最終結果は `player_game_fielding` の診断列と併せて確認する。

## 1球の全野手を確認

```sql
SELECT
    c.game_id,
    c.count_seq,
    c.fielder_position,
    c.fielder_id,
    c.lane_eligible,
    c.distance_eligible,
    c.evaluated,
    c.air_samples,
    c.height_ok_samples,
    c.reachable_samples,
    c.risk_ok_samples,
    ROUND(c.max_air_margin_sec, 4) AS max_air_margin_sec,
    c.required_margin_sec,
    ROUND(c.max_air_margin_time_sec, 4) AS max_air_margin_time_sec,
    ROUND(c.max_air_margin_height_m, 4) AS max_air_margin_height_m,
    c.last_evaluated_time_sec,
    c.end_reason,
    c.interception_catch_type,
    c.interception_error_type,
    c.interception_time_sec,
    c.selected,
    c.selected_path,
    b.first_bounce_time,
    b.fence_impact_time,
    b.result
FROM player_game_fielding_candidate AS c
LEFT JOIN player_game_batting AS b
  ON b.game_id = c.game_id AND b.count_seq = c.count_seq
WHERE c.game_id = 15658
  AND c.count_seq = 176
ORDER BY c.fielder_position;
```

## 外野単打となった球で、全外野手の失敗段階を集計

単打を拾った担当野手だけでなく、その球のLF・CF・RFすべてを対象とする。
「1球×1外野手」の件数なので、打球件数とは区別する。

```sql
WITH classified AS (
    SELECT
        c.fielder_position,
        c.selected,
        CASE
            WHEN c.evaluated = 0 THEN c.end_reason
            WHEN c.air_samples = 0 THEN 'no_air_samples'
            WHEN c.height_ok_samples = 0 THEN 'no_catchable_height'
            WHEN c.reachable_samples = 0 THEN 'too_late'
            WHEN c.risk_ok_samples = 0 THEN 'risk_rejected'
            WHEN c.interception_error_type IS NOT NULL THEN 'fielding_error'
            ELSE 'air_conditions_passed'
        END AS diagnosis
    FROM player_game_fielding_candidate AS c
    JOIN player_game_batting AS b
      ON b.game_id = c.game_id AND b.count_seq = c.count_seq
    WHERE b.result = 'Single'
      AND b.fielder_position IN ('LF', 'CF', 'RF')
      AND b.launch_speed >= 25 AND b.launch_speed < 35
      AND b.launch_angle >= 25
      AND b.first_bounce_time IS NOT NULL
      AND b.fence_impact_time IS NULL
      AND c.fielder_position IN ('LF', 'CF', 'RF')
)
SELECT fielder_position, diagnosis, selected, COUNT(*) AS n
FROM classified
GROUP BY fielder_position, diagnosis, selected
ORDER BY fielder_position, n DESC;
```

## 記録件数の確認

```sql
SELECT game_id, count_seq,
       COUNT(*) AS fielder_rows,
       SUM(evaluated) AS evaluated_fielders,
       SUM(selected) AS selected_fielders
FROM player_game_fielding_candidate
GROUP BY game_id, count_seq
ORDER BY game_id DESC, count_seq
LIMIT 50;
```

正常な9人守備では `fielder_rows = 9`、実効的な担当野手は `selected_fielders = 1`。
候補診断は移行後の新規試合から記録されるので、過去試合と比較する際は対象範囲を揃える。
