# タイミング位置ずれ75%の試験

最新コードでは `CONTACT_TIMING_POSITION_SCALE = 0.75` を試験設定とする。
`src/domain/resolver/batting_resolver.rs` のこの定数を1.0へ変更すれば対照設定になる。
それぞれ再ビルドして試合を生成する。係数は実測による採用決定前の試験値。
保存済みの100%試合があれば、そのデータを改修前の比較範囲として使用できる。
既存DBは `migrations/upgrades/add_timing_position_scale.sql` を1回実行する。
新規DBは最新DDLを使用する。前回のactual_bat_angle_deg追加も適用済みであること。

スイングごとに適用係数をtiming_position_scaleへ保存する。旧NULLは未縮小の1.0、
見送りはNULLのまま。係数を変えた範囲を混ぜない。
新しいスイングのtiming_impact_x_mは**適用後の変位**。
元のadapted_timing_offset_secおよびtiming_errorは秒の時間差を保持する。
HLAのタイミング由来のバット向きには元の時間差を使う。
ただし接触種別が変わればMarginalContact専用の既存再抽選の有無も変わり得る。

接触限界、EV・HLA・VLAの式は変更しない。入力の接触位置と接触種別は変わる。
EVのタイミング由来の接触深さ補正も適用後の変位を参照するため、EVの結果も変わり得る。
追加の乱数抽選は行わないが、接触増加により後続処理と乱数列は変わり、
同じシードでも試合展開や投球数は一致しなくなる。

`sql/diagnostics/compare_timing_position_trial.sql` の4パラメータに前後の試合ID範囲を指定する。
1本目: Contact%、PA単位K率、ファウル率（全接触が分母）、平均EV/VLA、実際の適用係数。
2本目: 接触種別ごとの結果。構成を比較するときは種別ごとのnを合計する。
3本目: ContactType×EV×VLAの分布とファウル・安打数。ファウルを含む接触が分母。
既存compare_swing_protection.sqlで見逃し／空振り三振とコース別の率も確認できる。

まずContact%が増えて総K率が下がるか、その後、SolidContact・高EV・安打が増えすぎないかを見る。
診断の同一スイング群で得た63.90%と、別の試合群の実測値が完全一致するとは限らない。
実測の安打率や本塁打率まで確認してから75%を採用するか再検討する。

既存timing_contact_probe.sqlは記録の適用係数を考慮するよう更新済み。
100/75/50/0%は未縮小変位に対する絶対条件で、再現性は記録に適用された行で検査する。
以前のSQLを75%記録へ使うと再現条件を誤るため、更新版を使う。
