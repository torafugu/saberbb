# 保存済み接触から低EV不足を調べる

上下の狙いを±30 mmまで変えても低EV打球が増えなかったため、接触の分布と衝突応答を分けて確認する。保存済みの接触ベクトルと応答だけを読み、DB・接触探索・衝突計算は実行しない。Rust本体や係数、能力値、発生確率は変更しない。

## 実行

```sh
python3 tools/diagnostics/plan_contact_ev_audit.py \
  --input-dir ../saberbb_plan_aim_height_wide_sample \
  --output-dir ../saberbb_plan_contact_ev_audit
```

選手・試合の再生成、`SABERBB_DB` の設定は不要。出力先は新しいフォルダを指定する。入力は傾斜・yaw・上下狙いの診断出力（metadata version 1/2/3/4）に対応する。

入力フォルダの metadata、sample、states、summary、EV/VLAヒストグラム、kinematics を読む。投球manifestのハッシュとキー、全条件の存在、保存EV/HLA/VLAとベクトル、元集計・ヒストグラム・運動学を照合する。接触フレーム、相対速度、保存インパルスによる速度の閉合も検証する。異常時は出力フォルダの作成前に停止する。

複数のseedフォルダも指定できるが、seed別に集計し、重なる投球を合算しない。同じseedのフォルダを複数渡すことはできない。±10 mmと±30 mmを同じseedで実行した場合、それぞれ別の出力先で診断する。

## 共有する6ファイル

- `plan_contact_ev_summary.csv`
- `plan_contact_ev_selection.csv`
- `plan_contact_ev_histogram.csv`
- `plan_contact_ev_pairs.csv`
- `plan_contact_ev_checks.csv`
- `plan_contact_ev_metadata.json`

元のDB、states JSONLの共有は不要。metadataには入力ハッシュ・元条件・探索/DB/衝突再計算がすべて0回であることを記録する。

## 見る順序

1. `selection` の `stratum=overall` で、時計不成立・接触なし・幾何学的接触・衝突採用/除外を確認する。各scenario/modelでtrialsを合計すると標本数になる。`saved_tilt` は一度だけ集計する。
2. `summary` の `cohort=geometry_contacts`、`stratum=normal_fraction` で、法線方向速度の小さい接触が存在するかを調べる。`normal_axial`、`normal_point_speed` で打点位置・速度との重なりも見る。
3. 同じ区分の `all_successful` と `fair_direction` を比較する。幾何学的には存在しても、端部/曲線範囲外の除外や打球方向によって分布から消えていないかを調べる。`selection` にも同じ区分と除外理由を保持する。
4. `histogram` は区分ごとのEV分布。平均だけで低EVの裾を判断しない。低EVは25 m/s未満、高EVは45 m/s以上、下向きはVLA 0度未満、高角度は50度以上。
5. `pairs` は同一接触の `normal_profile` → `finite_I0.00015_et0.4` の比較。摩擦等を含む応答でEVがどれだけ変わり、25 m/sの閾値を跨いだかを見る。`common_successful` と `common_fair` は別集団であり、条件間の狙い変更を直接ペア比較した表ではない。

各試行は全stratum・全modelへ重複して入る。一つのstratum内の区分だけを合算する。`summary` の `geometry_contacts` では contacts が全幾何学的接触数、successful_contacts がEV評価可能数となる。EV/VLAの平均・閾値件数は成功応答のみを使う。打球内割合には `all_successful` または `fair_direction` を選ぶ。

## 指標と解釈

| 指標 | 意味 |
| --- | --- |
| `relative_speed_ms` | 保存された球中心速度とバット中心線上の接触点速度との差の大きさ |
| `approach_speed_ms` | 上記相対速度の、球へ向く接触法線に対する接近成分（非接近は0） |
| `normal_speed_fraction` | 接近成分 / 相対速度。0に近いほど法線方向への接近が小さい |
| `bat_point_speed_ms` | バット中心線上の接触点速度 |
| `signed_axial_fraction` | 保存された最近点の長手座標 / 評価セグメント半長。芯からの距離ではない |
| `local_signed_b_over_r` | 接触時の円柱断面での符号付きb/R。端部では未定義 |
| `normal_only_ev_ms` | 同じ応答の法線インパルスだけを入射速度に加えた速度の大きさ |
| `tangential_delta_ev_ms` | 保存EV − 上記normal-only EV |
| `ball_rotation_change_j` | 保存された球回転エネルギーの衝突後 − 衝突前 |
| `total_loss_j` | 保存された球＋バット系全体の散逸エネルギー |

相対速度比は中心線上の速度による幾何学指標で、回転する球・バット表面のすべり速度ではない。相対速度が0なら比は未定義。接触なし・時計不成立でも接触指標は未定義とし、0で埋めない。

すべての平均に `n_` 列を付け、未保存/未定義なら `n=0`、平均は空欄にする。normal_profileには保存されていない散逸・回転エネルギーを0として追加しない。球回転エネルギーの変化は負にもなり得る。系の散逸全体を、球回転へ移ったエネルギーとは解釈しない。

normal-only EVは、結合応答の法線インパルスを固定した分解であり、摩擦なしで解き直した結果ではない。摩擦なしモデルとの比較にはpairsを使う。

法線速度比の小さい接触でも、入射球の接線方向速度が残ればEVが低くなるとは限らない。比だけで「弱い当たり」を定義せず、打点速度・インパルス・出力ベクトルと合わせて判断する。

この診断は保存データの整合性を確認するもので、最初の接触を正しく探索できたかを再検証しない。小標本の感度診断であり、実測へのフィットや本体パラメータの推奨は行わない。
