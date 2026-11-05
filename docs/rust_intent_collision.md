# 固定した打球意図の接触へ衝突応答を接続する

Rustの`execution_contact`をPythonで照合した後、既存の有限半径バット応答を接続する。
監督・コーチの指示と打者の選好、予測から生成した両意図の狙い・開始姿勢は固定したまま、
EV・VLA・回転数と下向き／高角度の割合を比較する。

今回の応答計算はPython診断のみ。Rustのライブ打撃処理への適用、Rust版の応答移植、係数校正は行わない。
幾何学的接触までの契約は[実行・接触診断](rust_execution_contact.md)、戦略階層は
[観測更新と打者戦略](observed_batting_strategy.md)を参照。

## 固定したモデルと入力

`rust_intent_collision_probe.py`は、各投球について先に`compare_execution()`を実行する。
標準ノイズ・観測更新・監督／打者の決定・両意図の計画・開始姿勢・接触探索が一致してから応答を評価する。
保存値の不一致はエラーにし、都合のよい接触候補へ選び直さない。

応答モデルは既存診断の`finite_I0.00015_et0.4`をそのまま使う。

| 項目 | 設定 |
| --- | --- |
| 質量・重心・横慣性 | 既存の`bat_vibration_profile`テンプレート |
| 反発係数 | 既存のfull_speed位置別曲線。範囲外へ外挿しない |
| 軸慣性 | 0.00015 kg m² |
| 摩擦係数mu / 接線反発et | 0.2 / 0.4 |
| バット半径 | 既存接触有効半径0.07 m − 球半径0.036 m |
| 球の入射速度 | 保存された実球速度。応答評価側だけで使用 |
| バット接触点速度 | 照合済みRust接触幾何の値 |
| バット角速度 | 固定開始姿勢のsin²パルスを接触時刻で評価。ピーク値で代用しない |
| 球の入射回転 | 同じgame_id/count_seqの保存投球スカラーから既存方式で再構成 |

球の回転は`player_game_pitching.spin_rate/spin_angle/spin_efficiency`と保存入射速度から再構成する。
既定の`gyro_sign=0`は未知のジャイロ成分を省略し、横成分をrpm×efficiencyとする仮定。
`--gyro-sign -1`／`--gyro-sign 1`は別の感度シナリオとして実行できる。実測3D回転ではなく、飛翔中の回転減衰も扱わない。
状態JSONと投球スカラーは同じ読み取り専用トランザクションから取得する。

側面接触で曲線範囲内かつ接近中のものを評価する。
端部・曲線範囲外・非接近・無効回転・エネルギー増加となる応答目標などは、理由を記録して未評価とする。
EVや回転を調整して成功扱いにしない。

応答ごとにmu=0の有限半径モデルも計算し、独立した法線衝突モデルとの出射速度一致と入射回転不変を確認する。
両モデルの運動量・角運動量・エネルギー・反発条件・摩擦円錐の恒等式を許容差1e-7以内で検査する。

## 実行

PR #77以降の`execution_contact`が保存された同じDBを使える。新しいRustフィールドやDB移行は不要。
DBパスは環境変数`SABERBB_DB`から取得する。CLIに`--db`は設けない。
以下は既に実行・接触の一致を確認したゲーム範囲の例。

```sh
python3 tools/diagnostics/rust_intent_collision_probe.py --first-game-id 26650 --last-game-id 27009 --output-dir ../saberbb_rust_intent_collision
```

接触探索のPython照合と、対象接触につき2回の有限半径応答計算を行う。まず小さなID範囲で実行時間を確認してもよい。
出力先は新しいフォルダを指定する。DBも既存出力も上書きしない。

保存回転が無効なら既定では停止する。欠損・重複した投球キーもエラーにする。
無効なスカラーを明示的に除外して進める場合だけ、別フォルダで次のオプションを使う。

```sh
python3 tools/diagnostics/rust_intent_collision_probe.py --first-game-id 26650 --last-game-id 27009 --output-dir ../saberbb_rust_intent_collision_exclude --invalid-spin-policy exclude
```

この場合もゼロ回転へフォールバックせず、`invalid_spin.csv`に投球キーと理由を残す。
除外方針とジャイロ符号はmetadataへ保存する。

## 出力の読み方

ファイル名の先頭はすべて`rust_intent_collision_`。

| 出力 | 内容 |
| --- | --- |
| summary.csv | 意図・監督指示・打者選好別の接触／応答件数、EV・VLA・回転、角度の割合 |
| pairs.csv | 同一投球のContactFirst→LineDriveの接触・フェア方向の増減と共通集団での差 |
| status.csv | 意図・selected別の応答状態。端部・曲線範囲外などの選択バイアスを確認 |
| coverage.csv | 状態件数、照合計画、再探索、回転参照投球、応答計算数と状態別件数 |
| checks.csv | Rust/Python照合、法線対照、物理恒等式の最大残差 |
| metadata.json | モデル、定義、入力コード・方針・DB参照行のハッシュ、仮定・制限 |
| details.csv | 投球キー、selected、狙い、幾何／応答状態と個別EV・VLAなど |
| invalid_spin.csv | 明示除外した無効回転の投球キー。正常ならヘッダーのみ |

最初は**summary・pairs・status・coverage・checks・metadataの6ファイル**を共有する。
無効回転を除外した場合はinvalid_spinも共有する。detailsは個別追跡時に使う。

`all_plans`は両比較候補、`selected_plans`は監督／打者が選択した意図のみ。
overallと予測高さ別（0.6／0.9／1.2 m境界）も重複するため、行を足して全体件数にしない。
既定設定では選択意図が全件ContactFirstでも、LineDriveの比較候補は評価される。

| 指標 | 分母・定義 |
| --- | --- |
| geometry_pct_per_searched | 幾何学的接触／探索した計画。未収束・開始時重なりも分母に含む |
| successful_pct_per_searched | 応答成功／探索した計画。接触率とは区別する |
| downward / high_vla / line_window_pct_per_searched | フェア方向かつ指定角度の応答件数／探索した計画 |
| downward / high_vla / line_window_pct_among_fair | 指定角度の応答件数／成功したフェア方向の応答 |
| mean_successful_* / mean_fair_* | 全成功応答／フェア方向応答の平均。未定義値は各n_*を分母にする |
| *_net_events_per_pitch_pp | 同一投球の後−前のイベント数差／全比較投球。時計不成立も分母に含む |
| common_fair_* | 両意図とも応答成功かつフェア方向だった同一投球だけの差 |

フェア方向は`abs(HLA)<45°`、下向きはVLA<0°、高角度はVLA≥50°。
line_windowは10°≤VLA<30°の診断用窓で、実測のライナー分類ではない。
接触から応答未評価となったものを空振りやEV=0に置き換えない。平均が欠けた場合も0と解釈しない。

接触率、全探索あたりのイベント割合、共通フェア集団での変化を併せて見る。
フェア打球だけの平均改善は、悪い接触が端部や曲線範囲外として除かれた結果でも起こり得る。
同一投球ペアは独立ではなく、ゲーム・選手の依存もある。この段階で有意差や実野球の最適戦術は断定しない。
Statcast構成比の調整、飛翔・地面・野手・得点結果の再計算は行わない。

固定した両意図の差の安定性は、[ゲーム単位の対応付きbootstrap](rust_intent_game_bootstrap.md)で確認できる。
元の診断フォルダに生成済みのdetails CSVも使うため、DB再読込や衝突再計算は不要。

## 検証

```sh
python3 -m unittest discover -s tests/diagnostics -p 'test_rust_intent_collision_probe.py'
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
```

接触時パルス、入力状態の不変性、端部／曲線範囲外／非接近の扱い、分母と同一投球ペアの集計、
監督指示と打者選好の分離、無効時計の非フォールバック、保存値改変時の応答前停止、
回転の欠損／重複／無効値、環境変数CLI、読み取り専用DBと出力非上書きを確認する。
