# 守備の捕球診断ログ

`player_game_fielding` の初回守備記録（通常 `seq = 1`）に、
`process_fielding()` が選択した実効的な捕球・拾球の情報を保存する。
診断用の取得は値のコピーだけで、捕球判定・乱数消費・試合結果を変更しない。

## 導入

- 新規DB：`migrations/ddl/player_game_fielding_table.sql` を使用する。
- 既存DB：`migrations/upgrades/add_fielding_catch_diagnostics.sql` を1回実行する。
  テーブルを再作成せず、既存行と主キーを維持する。適用済みDBに再実行しない。
- 既存行や併殺の2送目など、初回捕球の診断情報を持たない記録では追加列は `NULL`。
- 過去の試合の値は復元されない。移行後に実行した試合から記録される。

## 列

`game_id`、`count_seq`、`seq` は既存の主キーを使用する。

| 列 | 意味 |
|---|---|
| `selection_path` | `interception` / `covering` / `final_pickup` |
| `interception_fielder_position` | 選ばれた捕球・拾球担当野手の位置。後続の送球先とは独立 |
| `catch_type` | `DirectFly` / `BounceCatch` / `FinalPickup` / `Fumble` |
| `error_type` | 選ばれた野手の `Fumble` / `Passed`。エラーなしなら `NULL` |
| `catch_time_sec` | 打球発生から捕球・拾球処理完了までの秒数。処理遅延があればそれも含む |
| `catch_z_m` | 判定が返した捕球高さ（m）。拾球・ファンブルでは実装上0となる場合がある |
| `waiting_time_sec` | 判定が返した到達余裕時間（秒）。拾球処理遅延は含まない。ファンブル等では0の場合がある |
| `first_bounce_time` | 打球の最初の着地時刻（秒）。着地情報がない場合は `NULL` |

### 選択経路と捕球種別の違い

- `interception`：候補野手の時刻別探索から選択。着地後の `BounceCatch` もあり得る。
- `covering`：最初の野手が `Passed` となり、後続のカバー担当を選択。
- `final_pickup`：候補野手の探索で成立せず、最終拾球の探索から選択。

`covering` では **カバー担当の値** を保存する。最初に球を逸らした野手の
`Passed` が、カバー担当の `error_type` に転記されるわけではない。
`selection_path` が `NULL` なら診断未記録。`error_type` 単独では未記録と正常処理を区別できない。

これらは選択された結果のログであり、候補から除外された野手や各時刻の却下理由は記録しない。

## 1球を確認

```sql
SELECT
    b.game_id,
    b.count_seq,
    f.seq,
    b.launch_speed AS ev_ms,
    b.launch_angle AS vla_deg,
    b.result,
    f.selection_path,
    f.interception_fielder_position AS fielder_position,
    f.catch_type,
    f.error_type,
    f.catch_time_sec,
    f.catch_z_m,
    f.waiting_time_sec,
    f.first_bounce_time,
    f.time_to_field,
    ROUND(f.catch_time_sec - f.first_bounce_time, 4)
        AS catch_minus_first_bounce_sec
FROM player_game_batting AS b
JOIN player_game_fielding AS f
  ON f.game_id = b.game_id
 AND f.count_seq = b.count_seq
WHERE b.game_id = 15558
  AND b.count_seq = 160
ORDER BY f.seq;
```

`catch_minus_first_bounce_sec` が小さくても「ギリギリ空中捕球できなかった」とは限らない。
`final_pickup` は原則として着地・フェンス衝突時刻から探索するため、
先に `selection_path` と `catch_type` を確認する。
`waiting_time_sec` は選択された地点での余裕であり、空中捕球を断念した時刻の余裕ではない。

## 外野の経路別集計

```sql
SELECT
    f.interception_fielder_position AS fielder_position,
    f.selection_path,
    f.catch_type,
    f.error_type,
    b.result,
    COUNT(*) AS n,
    ROUND(AVG(f.waiting_time_sec), 3) AS avg_waiting_time_sec
FROM player_game_fielding AS f
JOIN player_game_batting AS b
  ON b.game_id = f.game_id
 AND b.count_seq = f.count_seq
WHERE f.seq = 1
  AND f.selection_path IS NOT NULL
  AND f.interception_fielder_position IN ('LF', 'CF', 'RF')
GROUP BY f.interception_fielder_position, f.selection_path,
         f.catch_type, f.error_type, b.result
ORDER BY fielder_position, n DESC;
```
