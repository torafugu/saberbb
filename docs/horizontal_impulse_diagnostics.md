# 水平速度と力積の分解

b/Rが大きい接触のファウル方向が、前方への速度不足か横方向速度の強さかを調べる診断。
衝突式・係数・軌道探索は変更せず、中心線/有限半径ソルバーの戻り値に法線/接線力積ベクトルを追加する。

## 座標と分解

保存されたワールド座標の+Yが前方、Xが横、Zが上。HLAはatan2(X,Y)。
打者の左右に合わせたミラー正規化は行わない。

`outgoing = incoming + normal_delta + tangent_delta`

normal_delta/tangent_deltaはそれぞれソルバーが実際に使用した力積ベクトルを球質量0.145kgで割った値。
基準source_constant_mass_normalは保存済み法線速度変化を使用し、接線変化はゼロ。
normal_profileは法線モデルのスカラー力積を使用する。
この診断の質量はゲーム診断の球質量であり、別の文献実験用プリセットに適用するものではない。
全成功接触で速度の閉合と接線変化の法線直交性を検査し、1e-7を超える不一致は停止する。

normal_onlyはincoming+normal_deltaという中間ベクトル。有限半径モデルでは
法線力積自体が接線力積と連成するため、摩擦ゼロで独立に解き直した予測ではない。
中間方向→最終方向はベクトル成分の分解であり、因果寄与やエネルギーの独立分配を意味しない。
角度やEVは非線形なので、法線と接線の角度寄与を加算しない。

最終水平速度は以下に分類する。

- nonforward: Y≤0。
- forward_lateral_dominant: Y>0かつ|X|≥Y。
- forward_fair_cone: Y>|X|。
- undefined_horizontal: 水平速度の大きさ≤1e-12 m/s。

従来のfinal_directionは|HLA|<45度をフェア方向とし、境界±45度はファウル方向。
極小水平速度では方向が不安定なのでundefined_horizontalを先に確認する。
`outgoing_forward_margin_ms = outgoing_y - abs(outgoing_x)`。
正ならフェア方向の円錐内、負なら外側。平均速度の符号から個々の打球の分類を決めない。

## 出力

- `finite_barrel_horizontal_coverage.csv`: 幾何接触についてモデルの成功/未評価件数。接触なしの試行は含めない。
- `finite_barrel_horizontal_summary.csv`: 全体/局所b/R×法線z別、法線のみの中間方向・最終方向・水平速度分類ごとの件数と平均。
- `finite_barrel_horizontal_checks.csv`: 速度閉合・直交性の最大残差（m/s）。
- `finite_barrel_horizontal_examples.csv`: b/R≥0.75の最初のモデル応答行。game_id/count_seq、幾何、全速度成分を含む。

incoming/normal_delta/tangent_delta/normal_only/outgoing/bat_point_velocityのX/Y/Z成分はm/s。
normal/axisは無次元の単位ベクトル。EVはm/s、VLAは度。
summaryは各項目にn_*を併記する。outcome_basisはsourceの基準応答または各モデル名。
sourceはモデル対象外の端面・曲線範囲外も応答があれば記録する。各モデルは成功した接触だけであり、
未評価値を補完しない。集合が異なる場合のモデル間平均を同一接触比較と見なさない。

overallとimpact_normal_z、scope、variant、モデルは重複するため合算しない。
例は全variant/modelを横断した先頭の該当応答で、代表標本や極端値ランキングではない。
既存--sample-limitと同じ上限を別枠で使用する。0を指定すると例はヘッダーのみ。

## 実行と確認順

同じ観測更新CSV・元DBから診断だけを再実行する。未作成の出力先を指定する。

```sh
python3 tools/diagnostics/finite_barrel_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_finite_horizontal \
  --pitch-db "/path/to/saberbb.db" \
  --gyro-sign 0
```

1. coverageと既存モデル成功件数、checksを確認する。
2. b/R≥0.75の応答をnonforwardとforward_lateral_dominantに分ける。
3. incoming_y・normal_delta_y・tangent_delta_yとX成分を比較する。
4. 同じ行のaxisとnormalで、バット姿勢・法線方向との関連を調べる。
5. examplesの個別キーでベクトルの閉合を再確認する。

基準応答から既に存在する偏りと、接線応答による変化を区別する。
この出力だけで姿勢モデルの不具合や、実測との差の原因を断定しない。
