# 接触判定の診断ログ

`player_game_batting` は、安打・アウト・ファウルだけでなく、空振り・空振り三振でも次の診断値を保存する。シミュレーションの計算式・閾値・乱数消費は変更しない。

既存DBはアプリ更新後の試合保存前に、次のSQLを**一度だけ**適用する。

```sh
sqlite3 /path/to/database.sqlite < migrations/upgrades/add_batting_contact_diagnostics.sql
```

既存行を削除・再計算しない。追加列は過去の行と見送りでは `NULL`、計算したスイングでは実測値（0を含む）になる。新規DB用DDLにも同じ列を追加してある。DDLの `DROP TABLE` を既存DBに対して実行しない。

| 列 | 意味 | 単位 |
|---|---|---|
| adapted_horizontal_offset_m | `adapt_to_pitch` 後の横方向誤差 | m・符号付き |
| adapted_vertical_offset_m | 同上の縦方向誤差 | m・符号付き |
| adapted_timing_offset_sec | 同上のタイミング誤差 | s・符号付き |
| timing_impact_x_m | スイング速度 × 補正後タイミング誤差 | m・符号付き |
| swing_error_x_m | スイング実行による追加横誤差 | m・符号付き |
| swing_error_z_m | スイング実行による追加縦誤差 | m・符号付き |
| contact_offset_x_m | 補正後横誤差 + 実行横誤差 + タイミングによるずれ | m・符号付き |
| contact_offset_z_m | 補正後縦誤差 + 実行縦誤差 | m・符号付き |
| thickness_offset_m | バットの厚み方向への最終射影 | m・絶対値 |
| length_offset_m（既存） | バットの長さ方向への最終射影 | m・絶対値 |

既存の `timing_error` も、空振り時に補正後のタイミング誤差を保存するようになった（従来は既定値の0）。スイングでは `adapted_timing_offset_sec` と同じ値。角速度やHLAなど、打球生成に依存する既存列の空振り時の扱いは従来通り。

## ミスの原因別集計

補正前の値は `player_game_pitching` にある。補正後との比較には `game_id, count_seq` でJOINする。以下は追加後の実測スイングだけを集計する。長さと厚みが両方超過した場合、現行の判定順に合わせて長さミスに分類する。閾値と等しい場合は超過ではない。

```sql
SELECT
    CASE
        WHEN length_offset_m > 0.16 THEN 'length_miss'
        WHEN thickness_offset_m > 0.070 THEN 'thickness_miss'
        ELSE 'contact'
    END AS contact_group,
    COUNT(*) AS n,
    ROUND(AVG(ABS(adapted_timing_offset_sec)) * 1000, 3) AS adapted_timing_ms,
    ROUND(AVG(ABS(adapted_horizontal_offset_m)) * 100, 2) AS adapted_horizontal_cm,
    ROUND(AVG(ABS(adapted_vertical_offset_m)) * 100, 2) AS adapted_vertical_cm,
    ROUND(AVG(ABS(timing_impact_x_m)) * 100, 2) AS timing_impact_cm,
    ROUND(AVG(ABS(swing_error_x_m)) * 100, 2) AS swing_error_x_cm,
    ROUND(AVG(ABS(swing_error_z_m)) * 100, 2) AS swing_error_z_cm
FROM player_game_batting
WHERE adapted_timing_offset_sec IS NOT NULL
  AND adjusted_contact_type <> 'Take'
GROUP BY contact_group;
```

符号付きの成分には相殺があり、各成分の絶対値平均を足しても最終誤差の絶対値平均には一致しない。
