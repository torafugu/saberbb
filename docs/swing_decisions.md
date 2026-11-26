# 見送り判断と2ストライク対応

既存DBには `migrations/upgrades/add_swing_decision_diagnostics.sql` を1回実行し、
新しい試合を生成する。旧行の `decision_*` はNULLのままで、過去を再構成しない。
新規DBは最新DDLを使用する。

変更は打者の振る／見送る判断だけ。HLA・VLA・接触限界・length missの判定は変更しない。
ゾーン外距離d（正規化ゾーン座標）をスコアに加える旧処理を、`-2*d` に変更する。
2ストライクでは積極性を維持し、Patient/Takeの見送り補正を0へ戻す。
追加の防御補正は `1.75*exp(-12*d)`。ゾーン内で1.75、遠いボールでは急速に減衰する。
以前の `0.35/(1+4*d)` よりゾーン内のスコアを1.40高める。
予想との不一致による負の補正（zone_similarity + pitch_similarity）は2ストライクで1/4へ弱める。
カウント自体の既存値、球種補正、得意コース補正は維持する。
これらの係数は初期設定であり、実測に適合済みの値ではない。

`decision_count_status` は投球前カウント（C12なら1ボール2ストライク）。
location_x/yは実コース。count/approach/protection/zone/pitch/anticipation/aptitude_factor
の和がscore、sigmoid(score)がprobability、swungが実際の抽選結果（0/1）。
診断は同じ計算結果を保存し、乱数抽選は従来通り1回。過去と振る球が変わるため
後続の乱数列・試合展開は変わり得る。
現在の判断は実コースを参照する近似であり、打者の知覚コースの独立モデルではない。
PlateApproachのデフォルト候補がすべてAggressiveである既存設定は今回変更しない。

`sql/diagnostics/swing_decisions.sql` で投球前カウント×コース別のSwing%と予測値、
見逃し／空振り三振数、判断要因を確認する。zone_coreは両座標の絶対値0.8以下、
zone_edgeは残りのゾーン内、outside_nearはゾーン外距離0.25以下。
この区分は診断用で、判定や係数を切り替える境界ではない。
SQLiteの数値関数を使う。必要なら各CTE内へgame_id範囲条件を足し、異なる版を分ける。

確認するのは、ゾーン内・際どい球で2ストライク時にSwing%が上がること、
遠いボールへ行くほどSwing%が下がること、見逃し三振減少と引き換えに
空振り三振やボール球へのスイングが過剰に増えていないこと。
実際のSwing%は球種・打者・予想の構成にも左右されるため、単一の全体率へ合わせて調整しない。

## 防御補正の強化後の比較

前回の実測では2ストライクのゾーン内Swing%が67.56%であり、見逃しが残った。
新しい補正はその約68〜70%を約89〜91%へ上げる初期設定で、確率の下限や固定値ではない。
カウント・球種・得意コース・予想に応じた違いを維持する。
ゾーン内は中央・端とも同じ補正で、外側へ出ると連続的に減衰する。
d=0.10で約0.527、d=0.25で約0.087、d=0.50で約0.004。
診断区分のoutside_near内でも境界付近は振りやすく、遠い側は以前より振りにくくなる。
2ストライク以外は従来の計算を維持する。新しい乱数・DB列・マイグレーションはない。

既存データを残して改修後の試合を生成し、`sql/diagnostics/compare_swing_protection.sql`
の4パラメータへ改修前後の試合ID範囲を指定する。
PAはこのモデルで打席を終えるresultの件数（DoublePlayは1打席）で集計し、K率は三振/PA。
完全な試合群を範囲指定し、`decision_recorded_pitches = pitches` を確認する。
診断未記録の三振は見逃し／空振りへ分類できないため、記録不足の範囲を比較に使わない。
ゾーン内Contact%は2ストライク・ゾーン内の全スイングに対する接触（ファウルを含む）の割合。
従来の詳細SQLにもContact%を追加し、outside_near/farを別々に確認できる。
見逃し三振が空振り三振へ移るだけの場合、角度・接触モデルとは切り分けて次段階で検討する。
試合展開と乱数列は変わるため、同じシードでも球ごとの対応比較にはならない。

## カウント別のゾーン際見送り試験

`SABERBB_SWING_EDGE_TAKES=1` を指定した日程消化で、ゾーン際の
スイングスコアを下げる。既定値は0。補正の大きさはゾーンの境界で最大になり、
ゾーン内側は最も近い境界から0.20、ゾーン外側はゾーンからの距離0.50で
線形にゼロへ戻る。距離は正規化したゾーン座標を使う。補正は
`decision_zone_factor` に含めて記録し、元の打者方針・好みの補正に加算する。
この試験も実コースを参照する。

| 投球前カウント | 境界で引くスコア |
| --- | ---: |
| 3–0、3–1 | 0.80 |
| 2–0、2–1 | 0.65 |
| 0–0、1–0、1–1 | 0.45 |
| 0–1 | 0.35 |
| 2ストライク | 0.20 |

Windowsのコマンドプロンプトで、同じ開幕前DBのコピー2つを使う。
以下のDBパスは各コピーの実パスに置き換える。

```cmd
set "SABERBB_SCHEDULED_GAME_SEED=20261126"
set "SABERBB_PITCH_AROUND_0_2_EDGE_SCALE=0"
set "SABERBB_PITCH_LOCATION_ERROR_FLOOR=0"
set "SABERBB_PITCH_EXECUTION_FATIGUE=1"
set "SABERBB_PITCH_CONTROL_CAP=1.20"
set "SABERBB_PITCH_CONTROL_ABOVE_CAP_SCALE=0.25"

set "SABERBB_DB=C:\path\to\edge_takes_baseline.db"
set "SABERBB_SWING_EDGE_TAKES=0"
cargo run --release --locked -- --process 40
python tools/diagnostics/pitch_control_audit.py --season 2026 --count-endings --count-margins > swing_edge_takes_baseline_2026.json

set "SABERBB_DB=C:\path\to\edge_takes_trial.db"
set "SABERBB_SWING_EDGE_TAKES=1"
cargo run --release --locked -- --process 40
python tools/diagnostics/pitch_control_audit.py --season 2026 --count-endings --count-margins > swing_edge_takes_trial_2026.json
```

各DBに対して `sql/diagnostics/swing_decisions.sql` を実行すると、
実コースのゾーン区分と投球前カウントごとのスイング率・三振内訳が出る。
SQLiteのCLIを使わない場合は、それぞれの `SABERBB_DB` を指定した状態で
次のコマンドを実行し、出力名を条件ごとに分ける。

```cmd
python -c "import csv, math, os, pathlib, sqlite3, sys; db = sqlite3.connect(pathlib.Path(os.environ['SABERBB_DB']).resolve().as_uri() + '?mode=ro', uri=True); db.create_function('sqrt', 1, math.sqrt); db.create_function('pow', 2, math.pow); rows = db.execute(pathlib.Path('sql/diagnostics/swing_decisions.sql').read_text(encoding='utf-8')); out = csv.writer(sys.stdout, lineterminator=chr(10)); out.writerow([col[0] for col in rows.description]); out.writerows(rows)" > swing_zone_bands_2026.csv
```

JSONの `by_count_endings["1-2"]` では、見逃し・空振り三振と
2–2への進行を投球数で割って比較する。
