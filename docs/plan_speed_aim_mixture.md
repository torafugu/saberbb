# 速度と上下の狙いを組み合わせて評価する

中心狙いの速度混合だけでは下向き率が実測へ届かなかったため、速度×上下狙いの各組み合わせで接触を計算し、仮のスイング構成比でEV/VLA分布を比較する。打者戦略は別途決定するものであり、今回もゲームへランダムな速度/狙いを導入しない。

## 1. 組み合わせごとの接触計算

```sh
python3 tools/diagnostics/plan_speed_sweep.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_plan_speed_aim_sample \
  --sample-size 1000 \
  --sample-seed 0 \
  --tilts-deg 0 15 \
  --speed-ratios 0.4 0.6 0.8 1 \
  --aim-z-mm -30 0
```

選手・試合の再生成は不要。元データに対応するDBをSABERBB_DB（または--db）から読み取り専用で取得する。予測位置・時刻と角度実行誤差を共通にし、意図した速度と狙いを反映して各計画の開始姿勢を作り、前向きに接触を探索する。

2傾斜×4速度×2狙いの16条件＋保存計画対照。最大17,000探索で、時計不成立の投球では探索しない。中心狙いと100%条件も含めて再計算するため、以前の結果との対照を保持する。予測球中心より下を狙う符号は負。−30 mmは診断の感度条件で、推奨打撃位置ではない。

## 2. 構成比を混合してStatcastと比較

```sh
python3 tools/diagnostics/plan_speed_mixture_compare.py \
  --input-dir ../saberbb_plan_speed_aim_sample \
  --statcast-csv ../saberbb_finite_statcast_input/savant_data.csv \
  --output-dir ../saberbb_plan_speed_aim_mixture_comparison \
  --mix-aim-offsets \
  --weight-step 0.1
```

こちらは保存済みキャッシュだけを使い、DB・接触探索・衝突再計算は不要。同じ投球・時計有効集合・宣言された全条件・元集計等を照合してから混合する。

同じ傾斜/yaw内で、次の独立した2つの構成比を置く。

- 速度の構成比：通常速度を60〜100%とし、残りを低速3条件へ10ポイント刻みで配分（35通り）
- 狙いの構成比：−30 mm狙いの割合を0〜100%まで10ポイント刻みで変更（11通り）

2傾斜×35×11＝770混合と、単独16条件を比較する。通常速度の下限は--minimum-full-speed-weight、構成比の刻みは--weight-stepで変更できる。

結合スイング比率は `w(speed, aim) = w(speed) * w(aim)` とする。これは**速度選択と狙い選択を独立と置いた感度仮定**で、実際の打者の選択頻度や独立性を実証したものではない。出力分布を掛け合わせるのではなく、上の手順1で実際に計算した各結合条件の打球数へ、この試行重みを掛ける。接触・フェア方向の選択後に正規化する。

速度と狙いが連動する構成比を確認する場合は、通常の--mixtures-jsonで各結合scenarioの重みを直接指定する。この場合は--mix-aim-offsetsを付けない。自動的な最適化や実測へのフィットは行わない。

全指標を同時に判定する許容幅は、従来と同じ--tolerances-jsonで指定できる。未指定では合否を付けず分布差を示す。同梱のplan_speed_tolerances.example.jsonは仮の数値例で、実証済みの許容基準ではない。

## 共有する6ファイル

手順2の出力先から以下を共有する。手順1のファイルは手元に保持し、再共有は必要な場合のみとする。

- `plan_speed_mixture_summary.csv`
- `plan_speed_mixture_assessment.csv`
- `plan_speed_mixture_envelope.csv`
- `plan_speed_mixture_checks.csv`
- `plan_speed_mixture_metadata.json`
- `statcast_coverage.csv`

components CSVも保存し、試行の構成比と選択された打球内の構成比を確認できる。組み合わせが増えるため、--mix-aim-offsets時は全EV×VLA格子CSVの保存を省略する。total variationや領域割合は同じ格子で計算し、省略によって変わらない。通常の速度混合と明示JSONでは従来どおり全格子を保存する。

速度構成比・狙い構成比・速度比を区別する。複数指標が個別の到達範囲に入っても、同じ構成比で同時に成立するとは限らない。envelopeだけでなくassessment/summaryを確認する。実測の期間・リーグ・選手構成、バント未分類、最終フェア判定との違い等の比較上の制約は従来と同じ。
