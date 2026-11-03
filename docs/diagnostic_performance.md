# 有限半径診断の実行時間を短縮する

既定の全診断では、8モデル×複数観測条件の衝突、入力回転とゼロ回転の比較、接地後応答、
これまで追加した全診断集計を毎回計算する。傾きだけを確認する場合はこの全処理は不要。

## 今回の傾き診断だけを実行

```sh
python3 tools/diagnostics/finite_barrel_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_finite_tilt_fast \
  --pitch-db "/path/to/saberbb.db" \
  --gyro-sign 0 \
  --diagnostics tilt \
  --models finite_I0.00015_et0.4 \
  --variants observed_sigma_0.005_r0.9
```

同じ設定のtiltの結果は全診断から該当モデル/variantを抜き出した結果と一致する。
モデル/観測条件を絞ったので、他のモデルや誤差条件の感度はこの実行では評価しない。
normal_profileとfinite_mu0は法線応答・回転の対照チェックのため常に残す（上の例では計3モデル）。
source_constant_mass_normalの行は保存元の基準応答で、モデルを選択しても残る。

出力先は未作成のディレクトリにする。試合・選手・観測更新CSVの作り直しは不要。
--sample-limitは例の数の上限だけであり、試行数や主な衝突計算を減らす指定ではない。

## 診断の指定

--diagnosticsに複数種類を指定できる。

| 値 | 出力 |
|---|---|
| core | 従来のsummary/pairs/status/examples |
| spin | 入射回転対ゼロ回転のpairs、差の分布、EV×VLA |
| selection | 全接触からフェア方向への選別 |
| direction | 幾何×方向、EV×VLA×HLA、観測条件間遷移 |
| horizontal | 水平速度と法線/接線力積の分解 |
| tilt | 軸の傾きと法線の幾何・方向別応答 |

--diagnostics未指定なら従来どおり全種類を出力する。
core/spinのどちらも指定しない場合、集計に使わない接地後応答を計算しない。
spin未指定ならゼロ回転との再計算と比較集計を行わない。
未指定診断のCSVを空ファイルとして作らず、出力自体を省略する。
coverage/checks/metadataと、pitch-db使用時の不正回転レポートは常に出力する。
checksは実際に計算した範囲のチェックであり、省略した地面やモデルを検証済みとは扱わない。

--modelsはモデル名を指定。--axial-inertiasを変更する場合はその設定で作られる名前を使う。
既知でないモデル名は出力作成前にエラー。coreのpairsは選択済みの両モデルがある比較だけを出す。
--variantsは元CSV内の観測条件とレバー長の組を指定し、指定外の衝突計算・集計を省略する。
入力CSV全体の読取り・ハッシュ・保存値の監査・キー重複/全条件の完全性検査は維持する。
フィルタ外の破損CSVも検出するため、読み込み時間が指定割合に比例して減るとは限らない。

directionには条件間の結合が必要なので--variantsとの併用はエラーにする。
方向診断だけなら、モデルを絞って全variantを保持する。

```sh
python3 tools/diagnostics/finite_barrel_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_finite_direction_fast \
  --pitch-db "/path/to/saberbb.db" \
  --gyro-sign 0 \
  --diagnostics direction \
  --models finite_I0.00015_et0.4
```

metadata version=9のexecutionに診断種類、variant選択、元試行数、処理試行数、
地面計算の有無、ゼロ回転対照の有無を記録する。coverageのtrial_rowsは処理した試行数。
モデルとvariantを絞った出力を以前の全件coverageと同じ分母として読まない。
不正回転のDB照合は選択された評価接触に対して行うため、未選択投球の回転値までは検証しない。

## 実測した処理コスト

全診断をcProfileで測ると、衝突計算に加え、選別集計・過去の平均集計・回転ペア・
接地後計算が時間を使っていた。基準法線応答は1試行につき1回だけ準備し、
入力回転/ゼロ回転で共用するよう変更した。全出力既定モードの結果は維持する。

テスト用観測履歴を複製した600試行（100倍反復）、固定入射回転、例0件での計測。
全て同じ環境でcProfileを有効にした測定値。ユーザーDBやMacでの測定ではない。

| 設定 | 秒 |
|---|---:|
| 変更前・全診断 | 6.84 |
| 変更後・全診断 | 6.58 |
| tiltのみ・全モデル/全variant | 1.90 |
| tiltのみ・1モデル＋対照2種・1variant | 0.26 |

全診断モードは小幅短縮であり、大きな短縮は必要な診断・条件だけを選ぶことによる。
変更前と変更後の既定出力は、この入力の全CSVでバイト一致した。
tilt限定の4CSVも全診断の同じ結果とバイト一致し、条件限定は対応行の一致をテストした。
データ・条件・I/O・機種・プロファイラ負荷で時間は変わるため、40〜50分が何分になるとは保証しない。

## 後続の高速化

全診断を頻繁に必要とする場合は、次に衝突結果の保存・再利用やプロセス並列化を検討する。
現時点では計算結果キャッシュや並列実行は導入しない。比較集合の結合や浮動小数点の集計順を
変える前に、今回の対象限定モードで必要な解析を進められるか確認する。
