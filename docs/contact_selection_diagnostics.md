# 接触集合の選別による分布変化

Statcast比較で見えた低EV・高角度打球の不足が、モデル適用範囲の除外やフェア方向の
選別で生じるかを調べる。有限半径衝突式・係数・入射回転の設定は変更しない。

## 応答と集合を分けて読む

`outcome_basis=source_constant_mass_normal` は観測更新CSVの保存済み応答。
固定有効質量0.6kg、反発0.5の法線衝突であり、有限半径・入射回転モデルの予測ではない。
CSVの監査を通過したこの同じ値を、全接触・適用可能・採用集合で集計することで、
応答モデルを変えずに選別の影響を確認する。端面接触や曲線範囲外の値も、この基準では
利用できる場合がある。欠損はゼロや外挿で埋めない。

`outcome_basis=input_spin_model` は今回指定した回転を入力した実際の衝突モデルの応答。
両basisを比較する際は同じstageと接触数・欠損数を確認する。
失敗した接触の有限半径応答は生成しないため、全接触stageでのinput_spin_model行も
成功した接触のみの集計になる。全接触で有限半径予測が得られたと解釈しない。
回転未指定ならinput_spin_modelもゼロ回転設定である。

| stage | 集合 |
|---|---|
| all_geometry_contacts | 幾何上の全接触。応答が未定義のものもcontactsに含む |
| geometry_eligible | 元の接触が接近・側面・反発曲線範囲内。保存回転の有効性はまだ条件にしない |
| model_success | 入力回転を含めて当該モデルの衝突計算に成功 |
| model_fair | model_successのうち当該モデルでフェア方向 |
| reason:* | 幾何上の接触を適用可否の理由で排他的に分解。eligibleも含む |

上4段階は入れ子で重複するため合算しない。reason行の合計は全接触と一致する。
回転不正などは別途coverageのmodel_statusに現れる。
no_contactなど接触前に外れた試行はcoverageだけに記録し、打球値を作らない。
フェア方向は飛翔・接地後を含む厳密なフェア打球判定ではない。

## 追加出力

- `finite_barrel_selection_coverage.csv`: scope/variant/modelごとの全試行数、幾何上の適用理由、モデル結果。
- `finite_barrel_selection_summary.csv`: contacts、EV/VLAの個別有効数、両方有効な数と欠損数、平均、
  EV<25・EV≥45・VLA<0・VLA≥50・EV≥40かつVLA<0の件数と割合。
- `finite_barrel_selection_ev_vla.csv`: 各basis/stageの共通EV×VLA格子。非ゼロセルのみ。
  fractionの分母はjoint_contacts。空の端点は無限大、下限を含み上限を含まない。
- `finite_barrel_selection_predictors.csv`: 各集合の全体と上記領域別に、打点位置・バット接触点速度・
  法線z成分・法線速度比・局所b/Rの平均と有効件数。因果寄与の分解ではない。

summaryの領域割合は各指標が定義された接触を分母にする。EVのみ・VLAのみ・両方という
違いをn_*で確認する。joint_contactsとの差を欠損として明示する。
全スイング率ではなく各段階内の割合であり、scope/variant/modelを合算しない。
modelごとに選別が違うため、モデル間の同一接触比較には既存pairsを使用する。

## 実行

同じ観測更新CSVと元DBを使用する。試合の再生成は不要。

```sh
python3 tools/diagnostics/finite_barrel_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_finite_selection \
  --pitch-db "/path/to/saberbb.db" \
  --gyro-sign 0
```

出力先は未作成ディレクトリ。DB・元CSVは変更しない。
最初にcoverageとsummaryで全接触・理由別・モデル成功・フェアの件数を確認する。
次にsource_constant_mass_normalの同じ指標を段階間で比較する。
最後にmodel_successまたはmodel_fairの同じ集合でbasisを比較する。
これにより選別差とモデル応答差を分けて読む。

これは診断CSVの全接触であり、試合DBの全打球ではない。観測履歴が揃うスイングなどの
上流選別は残る。除外接触の基準応答から、未計算の有限半径打球がどうなるかは断定しない。
