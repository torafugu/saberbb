# 仮のスイング構成比とStatcastの比較

低速スイングの選択は将来の打者戦略が決定する。現在は、保存済みの速度条件を集計上で混合し、ある構成比を仮定したときに物理モデルが実測の打球分布を説明できるかを調べる。ゲーム中の乱数や選択確率は変更しない。

## 実行

```sh
python3 tools/diagnostics/plan_speed_mixture_compare.py \
  --input-dir ../saberbb_plan_speed_sample \
  --statcast-csv ../saberbb_finite_statcast_input/savant_data.csv \
  --output-dir ../saberbb_plan_speed_mixture_comparison
```

入力は計画速度診断のmetadata version=4。states JSONLを含む元の出力フォルダを指定する。DB・接触探索・衝突再計算は不要で、SABERBB_DBも使わない。出力先は新しいフォルダとする。対象scopeはall_swings、既定modelはfinite_I0.00015_et0.4。元の標本manifest・条件・集計・EV/VLAヒストグラム・運動学をキャッシュに照合してから混合する。時計不成立の投球集合が条件間で同じことも投球ごとに確認する。

既定では傾斜/yaw/上下狙いが同じ条件内で速度だけを混合する。100%速度のスイング割合を60〜100%と仮定し、残りを40/60/80%速度へ5ポイント刻みで配分する。速度比と構成比を区別する。8速度条件（傾斜0/15度×4速度）なら混合330条件と単独8条件を出力する。saved_tiltはキャッシュ検証に使うが混合しない。

60%という下限と5ポイント刻みは測定された頻度ではなく、感度確認の仮定である。`--minimum-full-speed-weight` と `--weight-step` で変更できる。weight-stepは1を割り切る値。混合数が10,000を超える設定は拒否する。比較した有限の格子外まで到達不可能と主張しない。

## スイング数ベースの混合

各条件の打球分布を先に正規化して平均しない。条件iのスイング構成比をw_i、共通の評価可能投球数をN、あるEV/VLAセルの打球数をC_iとすると、まず `sum(w_i * C_i / N)` を求める。その後、選択された打球全体の質量で割る。

例えばスイング構成比が50/50でも、フェア方向率が80/20%なら、その打球集団内の構成比は80/20になる。componentsのtrial_weightとselected_cohort_weightで確認できる。時計不成立は全条件共通のため、全試行に置いた構成比を評価可能試行にも適用できる。未評価を空振りに含めない。

比較対象はStatcastのtype=XかつEV/VLAが揃った打球。EVはmphからm/sへ変換する。バントは既定で未分類のまま含み、完全なラベルがあれば `--bunt-labels` で除外できる。欠損は0で埋めずcoverageへ記録する。シミュレーションのフェア方向は飛行後の最終フェア判定ではなく、期間・リーグ・選手構成の同等性もこの診断では保証しない。

## 許容幅と判定

許容幅を指定しない場合、assessmentはnot_configuredで、差分と到達範囲を示す。自動的にモデルを合格としない。

比較前に許容幅を決めた場合は、次の引数を加える。

```sh
  --tolerances-json tools/diagnostics/plan_speed_tolerances.example.json
```

同梱ファイルは書式と診断のための**仮の数値例**。低EV率±1ポイント、高EV率±5ポイント、下向き率±5ポイント、高角度率±3ポイント、EV×VLA格子のtotal variation 0.25以下としている。実証済みの許容基準・信頼区間ではない。採用する基準として適切かを事前に検討し、必要ならコピーして編集する。結果を見て変更した基準は探索的なものとして扱う。

指定した全指標を同じ構成比で満たした場合だけwithin_configured_tolerancesとなる。外れた指標をfailed_metricsへ出す。低EVだけ合っても、下向きや高EVが外れていれば合格にしない。空の打球集団はempty_cohortで合格しない。

envelopeは単独条件群と仮の混合群それぞれの、指標ごとの最小・最大を示す。各指標が範囲内でも、同じ構成比で同時に到達できるとは限らない。同時成立はassessmentを確認する。total variationの最小値だけから構成比を自動採用せず、どの範囲の構成比で各指標が改善するかを見る。

## 明示した構成比だけを比較

`--mixtures-json PATH` を指定すると、既定の330条件を生成せず、指定した構成比と単独条件を比較する。

```json
{
  "version": 1,
  "mixtures": {
    "example_90pct_full_10pct_speed60": {
      "tilt_15deg_yaw_0deg_aim_z_0mm_speed_r1": 0.9,
      "tilt_15deg_yaw_0deg_aim_z_0mm_speed_r0.6": 0.1
    }
  }
}
```

構成比は有限・非負で合計1。未知条件・重複JSONキーは拒否する。独自構成比では姿勢条件も混合できるが、各scenarioの意味を明示する。これは診断上の仮定であり、状況別戦略を再現したことにはならない。

## 共有する6ファイル

- `plan_speed_mixture_summary.csv`：接触・フェア方向率、EV/VLAの4領域、実測値、格子total variation
- `plan_speed_mixture_assessment.csv`：実測との差（パーセントポイント）、指定許容幅、同時判定
- `plan_speed_mixture_envelope.csv`：比較条件群の指標ごとの到達範囲
- `plan_speed_mixture_checks.csv`：保存キャッシュの照合誤差
- `plan_speed_mixture_metadata.json`：構成比・基準・元条件・入力ハッシュ・制約
- `statcast_coverage.csv`：実測の採否と欠損

`plan_speed_mixture_ev_vla.csv`（全格子）と `plan_speed_mixture_components.csv`（試行/打球の構成比）も保存する。必要な場合に追加共有する。各条件は同じ投球を共有し、重み付き期待値は独立な観測数ではない。頻度推定・有意差検定・自動フィットは行わない。
