# 見送り判断と2ストライク対応

既存DBには `migrations/upgrades/add_swing_decision_diagnostics.sql` を1回実行し、
新しい試合を生成する。旧行の `decision_*` はNULLのままで、過去を再構成しない。
新規DBは最新DDLを使用する。

変更は打者の振る／見送る判断だけ。HLA・VLA・接触限界・length missの判定は変更しない。
ゾーン外距離d（正規化ゾーン座標）をスコアに加える旧処理を、`-2*d` に変更する。
2ストライクでは積極性を維持し、Patient/Takeの見送り補正を0へ戻す。
追加の防御補正は `0.35/(1+4*d)`。ゾーン内で最大、遠いボールでは減衰する。
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
