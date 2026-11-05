# 監督・コーチの目的と打者の選好を分けた選択方針の評価

既存のContactFirst／LineDriveと凍結係数を使い、選択規則だけを比較する診断。
監督・コーチが物理的な狙い位置を直接指定せず、目的と打者の選好から意図を決め、意図を既存プランナーへ渡す境界を維持する。

| 監督・コーチの目的 | 打者の選好 | 選択する意図 | 理由 |
| --- | --- | --- | --- |
| LetBatterDecide | ContactFirst | ContactFirst | 打者の選好 |
| LetBatterDecide | LineDrive | LineDrive | 打者の選好 |
| PutBallInPlay | ContactFirst | ContactFirst | 監督の接触優先 |
| PutBallInPlay | LineDrive | ContactFirst | 監督の接触優先。打者の選好は別項目で保持 |
| SeekLineDrive | ContactFirst | LineDrive | 監督のライナー優先。打者の選好は別項目で保持 |
| SeekLineDrive | LineDrive | LineDrive | 監督のライナー優先 |

この6通りは、全対象打者に同じ選好・チームに同じ指示を仮定する比較シナリオ。
加えて`recorded`は、元データに記録された監督指示と打者の選好の組み合わせをそのまま使う。複数の選好が混在していればその混在を保つ。
選手IDから新しい性格・能力・選好を推定したり、投球結果に合うように個人設定を割り当てたりしない。

現在の決定規則では、6通りの均一シナリオは最終的に2種類の意図へ集約される。
同じ意図になったシナリオの結果と区間が一致するのは仕様であり、独立した追加の検証標本には数えない。
監督が打者に任せる場合と選好を上書きする場合の違いは`decisions.csv`で追跡する。

## 入力と実行

[seeded holdoutの生成・監査](seeded_intent_holdout.md)で作成した**監査フォルダ**を入力にする。
`rust_collision_response_details.csv`など監査の7ファイルが必要。bootstrapの共有用4ファイルだけでは投球ごとの元キャッシュを再検査できない。
既存のPython衝突キャッシュも同じ検査を経て読めるが、明示seedの生成証跡がないものにholdout確認済みの意味を付けない。

```sh
python3 tools/diagnostics/intent_selection_policy.py --input-dir ../saberbb_holdout_seed_2026110502_audit --output-dir ../saberbb_holdout_seed_2026110502_policy --replicates 2000 --bootstrap-seed 0
```

この処理はDBを開かないため`SABERBB_DB`の追加設定は不要。DBを読む元の監査スクリプトでは引き続き`SABERBB_DB`を使う。
入力は変更しない。新しい出力フォルダだけを作成し、既存フォルダの上書きは拒否する。
接触探索・衝突応答の再計算は行わず、Rust/Python照合済みキャッシュの整合性を再検査して集計する。
キャッシュの改変、欠落した候補対、凍結係数・狙いファイルの不一致は出力作成前に拒否する。

## 選択と評価の分離

選択関数が受け取るのは監督の指示と打者の選好だけ。既存のRustと照合するPythonの同じ`decide`規則を利用する。
この段階ではカウント・走者状況・能力に応じた新しい規則や、結果によるスコア最大化を追加しない。
ContactFirstは予測中心、LineDriveは世界座標での予測高さ0.6m未満なら中心、それ以上なら15mm下を狙う既存規則のまま。
予測情報に基づくプランを選んだ後、その候補に対応する診断結果を評価へ集計する。
実際の接触法線、接触成立、フェア判定、EV、VLA、回転数から候補を選び直さない。

`PutBallInPlay`という名前からフェア率が最大だとは判断しない。現実装はContactFirstを選ぶ明示規則であり、
接触率・フェア率・下向き率・高角度率・ライナー角度帯率を別々に比較する。
フェア率と接触率をまとめる効用関数や重みを、このholdoutに合わせて作る処理は含まない。

## 出力と分母

| ファイル | 内容 |
| --- | --- |
| intent_selection_policy_summary.csv | 各シナリオの絶対値・95%区間、常時ContactFirstとの差・差の95%区間 |
| intent_selection_policy_decisions.csv | 元の指示・選好、比較用の有効な指示・選好、選択意図・理由・投球数 |
| intent_selection_policy_height.csv | 予測高さ別の記述集計。区間推定・高さ別の自動選択は行わない |
| intent_selection_policy_coverage.csv | ゲーム数、比較可能な投球数、シナリオ数、区間が未定義の件数 |
| intent_selection_policy_checks.csv | 元の物理・照合検査、キャッシュ再集計、ゲーム別合計・選択件数の検査 |
| intent_selection_policy_metadata.json | シナリオ定義、生成証跡を含む入力metadata、入力・コードハッシュ、制約 |

`*_pct_per_pitch`の分母は、**両候補がキャッシュに記録された全投球数**。
探索できなかった計画、非接触、応答対象外もこの分母に保持する。応答対象外を物理的な空振りと断定しない。
これは全試合の全投球や全スイングを分母にした実測率ではなく、元監査で比較可能だった診断候補対の集計。
元の監査coverageにある欠損・対象外状態はmetadataから確認できる。
探索済み数を分母にする従来bootstrapの率とは分母が違うため、差の数値をそのまま並べて比較しない。

`successful`は衝突応答が成立した接触、`fair`は水平方向がフェア範囲に入る応答。
下向きはフェア方向でVLA<0°、高角度はVLA≥50°、ライナー角度帯は10°≤VLA<30°。安打・アウトや観測上のライナー分類ではない。
`*_pct_among_fair`と平均EV/VLA/回転数は各方針のフェア集団に条件付けるため、集団が変わる影響も含む。
常時ContactFirstとの差の単位は率ならパーセントポイント、それ以外はm/s・度・rpm。

ゲーム全体を復元抽出し、全シナリオに同じ抽出回数を使う。比率は件数合計から求め、試合別の率を単純平均しない。
検証済みmanifestの寄与0のゲームも抽出母集団に保持する。分母0の反復が1件でもあれば、その区間を出さない。
絶対値と差はそれぞれ区間・有効反復数を持つ。0差の対照でも、分母が未定義なら0に置き換えない。
区間は個別指標ごとで、多重比較補正はしない。選手・チームの試合間依存は解消されない。

ゲーム別合計の照合では件数を完全一致で検査する。EV・VLA・回転数の浮動小数点の累積値は、
投球順とゲーム単位で加算順が異なるため、合計差を対応する観測件数で割った残差を既存の許容誤差で検査する。
checksの該当項目には`.per_observation`を付ける。平均値の精度で照合し、件数不一致・非有限値・許容範囲を超えた差は引き続き拒否する。

共有する基本ファイルは`summary.csv`、`coverage.csv`、`checks.csv`、`metadata.json`の4つ。
選択理由を追う場合は`decisions.csv`、高さの内訳を見る場合は`height.csv`も使う。

## 次の判断

この診断は既存の選択規則を明示して比較するためのもの。ライブ打撃の変更、打席中の投球系列の変化、安打・得点・勝率、Statcast構成比の調整は評価しない。
既に評価したholdoutを使うため、この出力を見て新しい境界・重みを決める場合、その規則は新しい仮説として凍結し、未使用seedのholdoutを別途用意する。

## 検証

```sh
python3 -m unittest discover -s tests/diagnostics -p 'test_intent_selection_policy.py'
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
```
