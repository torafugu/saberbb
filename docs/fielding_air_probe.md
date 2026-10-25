# 全野手の空中捕球診断探索（0.01秒刻み）

## 導入

既存DBでは候補診断テーブルを作成済みであることを確認してから、
`migrations/upgrades/add_fielding_air_probe.sql` を **1回** 実行する。
未作成なら先に `add_fielding_candidate_diagnostics.sql` を実行する。
新規DBでは更新後の `migrations/ddl/player_game_fielding_candidate_table.sql` を使用する。

新しい試合の `player_game_fielding_candidate` 各行に `air_probe_*` を保存する。
既存行は `NULL` のままで、過去の球の再計算・補完はしない。
見送り・空振り・本塁打など、`process_fielding()` に入らない球は対象外。

## 診断探索の意味

- 方向・距離フィルタに関係なく、実際に守備へ渡された全野手を評価する。
- 通常の探索と同じ0.10秒から開始し、整数カウンタで0.01秒刻みの時刻を生成する。
- `estimate_ball_state_at_time()` による空中区間だけを対象とし、最初の着地またはフェンス衝突で終了する。
- 実行時の野手位置、走力、準備時間、捕球可能高さ、リスク設定を使う。DBの現在の能力値から事後計算しない。
- 捕球可能な地点を見つけても最後まで診断を続ける。複数の野手が捕球可能と判定される場合もある。
- 純粋な計算関数で、乱数・エラー判定・担当選択・試合結果へ影響を与えない。
- 軌道点は1球につき1回生成して野手間で共有する。全野手・全空中区間を調べる分の計算量は追加される。

「捕球可能」は高さ・移動時間・要求余裕の条件が成立したという意味。
エラーや他の野手との優先関係まで含めた、実際の捕球成功を保証するものではない。

通常判定の `evaluated` や `end_reason` は従来の候補選別と0.05秒刻みの結果を表す。
`evaluated = 0` でも新しい診断列は記録される。
通常の件数は早期終了まで、新しい件数は空中区間全体なので、件数比を直接比較しない。

## 追加列

| 列 | 意味 |
|---|---|
| `air_probe_step_sec` | 診断済みなら0.01。NULLは旧データなどで診断未記録 |
| `air_probe_air_samples` | 空中探索点数。0は診断したが対象区間がなかったことを示す |
| `air_probe_height_ok_samples` | 捕球可能な高さの点数 |
| `air_probe_reachable_samples` | さらに移動が時間内に間に合う点数 |
| `air_probe_risk_ok_samples` | さらに要求余裕を満たす点数。1を超える場合もある |
| `air_probe_max_margin_sec` | 捕球可能な高さの点での最大到達余裕。負値も保存 |
| `air_probe_max_margin_time_sec` | 最大余裕を最初に記録した時刻 |
| `air_probe_max_margin_height_m` | その地点の高さ |
| `air_probe_first_catch_time_sec` | すべての条件を最初に満たした時刻 |
| `air_probe_first_catch_height_m` | その地点の高さ |
| `air_probe_first_catch_margin_sec` | その地点の到達余裕 |

時刻・余裕は秒、高さはm。到達余裕は `探索時刻 - (準備時間 + 移動距離 / 走力)`。
最大余裕は捕球可能な高さの地点がない場合や、移動時間を有効に計算できない場合はNULL。
全条件を満たす地点がなければ `first_catch_*` の3列はNULL。
ゴロなど空中探索点がない場合も、診断済みならstep=0.01・各件数=0となる。

## 1球の全野手を確認

過去の16712/22には値が補完されないため、対象を新規試合のキーへ置き換える。

```sql
SELECT
    c.game_id, c.count_seq, c.fielder_position, c.fielder_id,
    c.lane_eligible, c.distance_eligible, c.evaluated, c.end_reason,
    c.selected, c.selected_path,
    c.risk_ok_samples AS normal_risk_ok_samples,
    c.air_probe_step_sec,
    c.air_probe_air_samples,
    c.air_probe_height_ok_samples,
    c.air_probe_reachable_samples,
    c.air_probe_risk_ok_samples,
    ROUND(c.air_probe_first_catch_time_sec, 4) AS first_catch_time_sec,
    ROUND(c.air_probe_first_catch_height_m, 4) AS first_catch_height_m,
    ROUND(c.air_probe_first_catch_margin_sec, 4) AS first_catch_margin_sec,
    ROUND(c.air_probe_max_margin_sec, 4) AS max_margin_sec,
    ROUND(c.air_probe_max_margin_time_sec, 4) AS max_margin_time_sec,
    ROUND(c.air_probe_max_margin_height_m, 4) AS max_margin_height_m,
    b.first_bounce_time, b.fence_impact_time, b.result
FROM player_game_fielding_candidate AS c
LEFT JOIN player_game_batting AS b
  ON b.game_id = c.game_id AND b.count_seq = c.count_seq
WHERE c.game_id = 16712 AND c.count_seq = 22
ORDER BY c.fielder_position;
```

## 除外された外野手にも捕球可能地点があった球を抽出

最終担当に選ばれたかどうかに関係なく、全外野手を対象とする。
方向条件も距離条件も不成立の場合、`end_reason`は方向条件を優先するので両フラグを見る。

```sql
SELECT
    c.game_id, c.count_seq, c.fielder_position, c.fielder_id,
    c.lane_eligible, c.distance_eligible, c.end_reason,
    c.selected, c.selected_path, b.result,
    c.air_probe_risk_ok_samples,
    ROUND(c.air_probe_first_catch_time_sec, 4) AS first_catch_time_sec,
    ROUND(c.air_probe_first_catch_height_m, 4) AS first_catch_height_m,
    ROUND(c.air_probe_first_catch_margin_sec, 4) AS first_catch_margin_sec
FROM player_game_fielding_candidate AS c
JOIN player_game_batting AS b
  ON b.game_id = c.game_id AND b.count_seq = c.count_seq
WHERE c.fielder_position IN ('LF', 'CF', 'RF')
  AND c.evaluated = 0
  AND c.air_probe_risk_ok_samples > 0
ORDER BY c.game_id, c.count_seq, c.fielder_position;
```

この条件は「フィルタを無視し、かつ0.01秒へ細分化した探索で成立」を意味する。
通常の0.05秒刻みでも捕れたかまでは断定できず、フィルタだけの影響とは分離されていない。

## 打球単位で件数を集計

1球を複数の外野手が捕れる場合の重複を避ける。
`excluded_but_probe_possible` と `normal_failed_but_probe_possible` は同じ球で両方成立し得るため、合算しない。

```sql
WITH by_ball AS (
    SELECT
        c.game_id, c.count_seq,
        MAX(CASE WHEN c.air_probe_risk_ok_samples > 0
                 THEN 1 ELSE 0 END) AS probe_possible,
        MAX(CASE WHEN c.evaluated = 0 AND c.air_probe_risk_ok_samples > 0
                 THEN 1 ELSE 0 END) AS excluded_but_probe_possible,
        MAX(CASE WHEN c.evaluated = 1 AND c.risk_ok_samples = 0
                  AND c.air_probe_risk_ok_samples > 0
                 THEN 1 ELSE 0 END) AS normal_failed_but_probe_possible
    FROM player_game_fielding_candidate AS c
    WHERE c.fielder_position IN ('LF', 'CF', 'RF')
      AND c.air_probe_step_sec IS NOT NULL
    GROUP BY c.game_id, c.count_seq
)
SELECT
    b.result,
    COUNT(*) AS balls,
    SUM(p.probe_possible) AS probe_possible,
    SUM(p.excluded_but_probe_possible) AS excluded_but_probe_possible,
    SUM(p.normal_failed_but_probe_possible) AS normal_failed_but_probe_possible
FROM by_ball AS p
JOIN player_game_batting AS b
  ON b.game_id = p.game_id AND b.count_seq = p.count_seq
GROUP BY b.result
ORDER BY balls DESC;
```

`normal_failed_but_probe_possible` は、通常探索を実行した野手で全条件を通る空中点がなく、
細かい診断探索では成立したケース。既存の粗い刻みの影響を調べる手掛かりになる。
捕球エラーで安打になったケースは、通常探索のrisk_ok_samplesが1なのでこの区分に入らない。

## 検証用ケース

16712/22の公開された位置・能力値と代表的なmax_height=20mによる回帰ケースでは、
通常探索の空中成立点0に対し、診断探索では3.02/3.03/3.04秒の3点が成立する。
当時のmax_height自体は保存されていないため、歴史上の1球を完全再現したテストではない。
このケースに加え、高さ不成立・負の余裕・リスク条件・フェンス衝突による終了・ゴロ・
候補除外後の診断成立・乱数列の維持・DB移行と保存をテスト対象とする。
