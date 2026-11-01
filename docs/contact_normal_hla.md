# 符号付き接触座標からのHLA試験モデル（version 2）

既存DBへ `migrations/upgrades/add_contact_normal_hla.sql` を1回実行し、再ビルドして
新しい試合を生成する。75%の位置ずれ係数、接触限界、EV・VLAの式を維持する。
旧データのhla_model_version=NULLはversion 1。新規打球は2。空振り・見送りではNULL。
新規スイングのsigned_length_offset_m / signed_thickness_offset_mに、絶対値化する前の
局所座標を保存する。接触判定では従来同様にその絶対値を使う。

角度theta、世界座標のずれx,zに対して、

```
length_signed = x*cos(theta) + z*sin(theta)
thickness_signed = -x*sin(theta) + z*cos(theta)
s = clamp(thickness_signed / 0.070, -1, 1)
n = (-sin(theta)*s, sqrt(1-s*s), cos(theta)*s)
reflected = (0,-1,0) + 2*n.y*n
rebound_hla = degrees(atan2(reflected.x, reflected.y))
HLA = wrap_to_180(handedness * (0.85*face_angle + rebound_hla))
```

接触判定と方向生成が同じprojection関数を使う。バット軸に沿う長さを円筒断面の
曲率へ入れず、符号付き厚みと姿勢から水平方向の反発を計算する。
全ContactTypeで元の時間差を使い、Marginal専用タイミング再抽選は削除する。
既存のapproach/pullの抽選は維持し、種別別のファウル確率や追加ノイズは導入しない。

これは**理想弾性反射の水平方向だけを使う試験的な近似**。
投球速度・バット移動速度・反発係数・端部形状を使う完全な衝突計算ではない。
EV・VLA・スピンは従来式なので、3成分が一つの衝突モデルから導出されるわけではない。
特にかすった接触は90度以上の後方打球になり得る。ファウルが増えすぎる可能性も含めて検証する。
この試験だけで物理的正しさやファウル率の適合を保証しない。

元時間差からのface angleは保持する。座標の変換と接触品質の式は維持するが、
Marginalの再抽選を削除するため乱数消費と以後の試合展開は変わる。
実際のプレーでは角度の違いでファウル・捕球・打席の長さも変わる。

## 確認手順

1. `sql/diagnostics/hla_components.sql` 更新版で新しい試合範囲を指定し、missing_inputs=0、
   hla_mismatches=0を確認する。version 1/2を識別して独立再現する。
2. `sql/diagnostics/compare_contact_normal_hla.sql` の4パラメータに前後の完全な試合ID範囲を設定。
   ContactType別の実ファウル率と打ち出し方向、左右別のHLA分布、EV/VLA分布・安打・HRを比較する。
3. `compare_timing_position_trial.sql` でContact%・PA単位K率も確認する。

同じ打球の角度だけを変えた実プレー比較ではなく、別々に生成した試合群の比較。
Contact%も後続の投球構成や乱数列が変わるため完全には一致しない。
HLAのabs>=45は打ち出し時点の方向で、result='Foul'（ファウルフライ捕球を含まない）とは区別する。

新版では元のタイミングと採用タイミングが同じなので、hla_componentsのoriginal_timing版と
current版は一致する。残した比較は旧版データとの原因分析用。
旧版へ戻す場合は親コミットのコードを別ブランチでビルドし、記録済みDB列を残す。
今回の試験を採用するか、移動バットを考慮した衝突モデルへ進むかは結果を見て決める。
