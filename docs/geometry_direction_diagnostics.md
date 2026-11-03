# 接触幾何と打球方向・観測条件の診断

目的: 弱い打球・高角度打球がファウル方向へ偏る条件を探る。
係数・衝突モデルは変更せず、既存のfinite_barrel診断の入力を再利用する。

## 出力

- `finite_barrel_direction_summary.csv`: 方向別・幾何区間別の件数、EV/VLA平均、
  低EV/高速/下向き/高角度/高速下向きの割合、HLA円周平均と合成ベクトル長。
- `finite_barrel_direction_joint.csv`: EV帯×VLA帯×HLA帯の同時度数。全接触方向を含むoverallのみ。
- `finite_barrel_observation_transitions.csv`: 同じ投球・同じレバー長について、
  prior_onlyおよびexact_observation_controlから各observed_sigma条件への接触状態の遷移。

scope/variant/outcome_basisごとに読む。source_constant_mass_normalは監査済みの元CSVの
固定質量法線応答。各モデル名は今回の回転設定を使った成功接触のみの応答。
sourceで幾何接触があっても応答が定義されなければ件数だけを残し、有効指標数は別記録。
モデル適用不能接触へ有限半径の値を補完しない。

stratumは以下の3種類。重複するため合算しない。

1. overall: 集合全体。
2. impact_normal_z: 局所絶対b/R帯×法線z帯。
3. axial_normal_azimuth: ノブからの打点位置帯×水平面内の接触法線角帯。

局所b/Rは接触時の凍結した無限円柱近似であり、運動全体の最接近距離ではない。
法線水平角はatan2(normal_x,normal_y)のワールド角。HLAそのものや打者視点の引っ張り角ではない。
水平法線が未定義の場合はundefined。左右打者のミラー正規化は行わない。
法線zもワールド座標の成分である。

方向は従来どおり|HLA|<45度をfair_directionとする。±45度はfoul_direction。
区間は下限を含み上限を含まないため、-45度を含むHLA帯は方向別の列でも区別する。
sourceと各モデルでフェア集合が異なる。実際の落下地点まで含めたフェア判定ではない。

HLAは算術平均しない。sin/cosから円周平均を計算し、合成ベクトル長を併記する。
相反する方向が相殺して平均方向を定義できないときは空欄。
合成ベクトル長が小さい集団の平均角だけで解釈しない。
EV/VLAや領域割合も各有効件数が分母。jointは非ゼロセルのみの件数であり、
EV/VLA/HLAが揃う接触に限定する。scope/variant/modelを合算しない。

## 接触から非接触への遷移

比較はgame_id/count_seq/lever_mをキーにする。CSVの行順には依存せず、一時SQLiteで結合する。
元ContactSourceによる全条件のキー・件数監査が終わった後に遷移を出力する。
変更前の接触幾何・EV/VLA帯・方向と、変更後の探索状態・応答状態・方向を記録する。
弱い打球と高角度の両方に属するものはlow_ev_and_high_vlaとして分ける。

contact→no_contactは、この診断の接触探索で接触が消えた試行を表す。
overlap_at_startやunresolvedを空振りとして合算しない。接触しても非接近で応答がない場合も区別する。
変更後に接触がなければ、変更後のb/Rや打球値は作らない。
遷移の応答は元CSVの固定質量法線応答であり、変更前の有限半径EVによる分類ではない。

誤差ゼロから観測誤差条件への比較は同一球の診断上の反実仮想であり、
実際の試合の接触が観測誤差で失われたことや、実測の弱い打球の不足原因を直接証明しない。
prior_onlyからの比較は観測による更新そのものの影響も含む。
上流の有効観測履歴を持つスイングという選別は残る。

## 実行

選手・試合・観測更新CSVの再生成は不要。同じ元DBを使い、未作成の出力先を指定する。

```sh
python3 tools/diagnostics/finite_barrel_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_finite_direction \
  --pitch-db "/path/to/saberbb.db" \
  --gyro-sign 0
```

最初にsummaryのoverallと既存selectionの成功・フェア件数の一致を確認する。
次にb/R×法線z、打点×水平法線角ごとにfair/foulの件数と打球指標を見る。
jointで方向・EV・VLAの対応を確認し、transitionsでは変更前の幾何ごとに
contact→no_contactの件数を全遷移数と比較する。
実データの集計前に、この診断だけから係数を調整しない。
