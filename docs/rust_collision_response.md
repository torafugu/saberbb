# 固定した衝突応答をRustの診断へ移植する

ゲーム単位bootstrapで確認した両意図のトレードオフを保ったまま、
Pythonの`finite_barrel_collision.solve()`と投球回転の再構成をRustへ移す。
狙い・開始姿勢・監督指示／打者選好・物理係数は変更しない。
今回も診断記録のみで、ライブ打撃の接触・打球・得点・試合乱数へ応答を戻さない。

## 構成と固定モデル

`finite_bat_collision.rs`は幾何・速度・回転・物理パラメータだけを受け取る純粋な応答計算。
`intent_collision_response.rs`は既存の観測更新後の計画と`execution_contact`へ接続する。
意図の選択や狙いの計画に、実球の終点・接触法線・打球結果を渡さない。

| 項目 | 内容 |
| --- | --- |
| モデル | finite_I0.00015_et0.4 |
| 質量／横慣性／重心 | 既存の0.885 kg、0.885×0.23² kg m²、基準点から0.564−0.71 m |
| 軸慣性／mu／et | 0.00015 kg m²／0.2／0.4 |
| 球 | 0.145 kg、半径0.036 m、慣性係数0.4 |
| バット半径 | 診断接触有効半径0.07 m − 球半径 |
| 反発係数 | 既存のfull_speed位置別16点を同じ範囲で線形補間 |
| バット角速度 | 開始姿勢からのsin²パルスを接触時刻で評価 |
| 入射回転 | 保存投球スカラーと入射速度から既存方式で再構成。gyro_sign=0 |

物理モデルの根拠・適用範囲は[既存Python衝突診断](rust_intent_collision.md)と同じ。
今回新しい係数の選択・校正はしない。
純粋な応答APIのパラメータは共通ケースの試験用にも使うが、本体の診断接続は固定モデルで生成する。

## 保存するフィールド

`player_game_contact_state.state_json.intent_collision`を任意フィールドとして追加する。
旧JSONはそのまま読み込め、DBのテーブル・INSERT文の変更や移行は不要。

| 記録 | 内容 |
| --- | --- |
| version / source | 1 / intent_collision_shadow_v1 |
| model / decision | 固定モデル、監督指示、打者選好、選択意図、理由 |
| spin | 元の投球スカラー、gyro_sign、再構成したベクトルまたは無効理由 |
| probes | 両意図それぞれのselected、診断エラー、応答評価 |
| evaluation.input | 応答に実際に渡した幾何、入射・バット速度／回転、全パラメータ |
| evaluation.response | 出射速度・回転ベクトル、EV/HLA/VLA、エネルギー・衝撃量・摩擦条件・物理checks |
| evaluation.control | 同じ入力でmu=0、et=0とした有限半径応答 |
| evaluation.control_checks | 独立した法線応答との速度差、入射回転不変の残差 |

時計不成立・接触なし・未収束・開始時重なり、端部・曲線範囲外・非接近・無効回転は
既存Python診断と同じ未評価状態を保持する。未評価を空振りやEV=0へ置換せず、別意図へ切り替えない。
無効回転のときも、先に端部・曲線範囲外などの幾何上の適用可否を判定する。

純粋な応答計算の入力エラーや物理恒等式の不一致は、該当probeのinvalid_reasonへ保存する。
試合を中断したり、EV・回転を補正して成功扱いにしない。Python監査では予期しない診断エラーを不一致として停止する。
適用条件で除外した場合はinput/controlがnull。応答目標が不成立の場合は入力とmu=0対照を保持し、responseに未評価理由を残す。
投球スカラーの非有限値はnullと無効理由を残し、ゼロ回転へ戻さない。

ログには両候補と対照応答も含むため、まず少数の新規試合で処理時間・記録量・coverageを確認する。

## 実DBでの一致確認

取り込み後のRustで、新しい試合を10件程度生成・保存する。
古い試合にはRust応答が記録されていないため、前回のCSVや既存DB行から補完しない。
次のゲームIDは例で、**今回生成した実際の範囲に置き換える**。

DBパスは環境変数`SABERBB_DB`から取得する。

```sh
python3 tools/diagnostics/rust_collision_response_audit.py --first-game-id 27010 --last-game-id 27019 --output-dir ../saberbb_rust_collision_response
```

監査は同じ読み取り専用トランザクションで状態と`player_game_pitching`のスカラーを読み、
Rust記録のスカラーが保存投球と一致することを確認する。
既存の観測・判断・計画・軌道・接触をPythonで照合してから、固定モデルの応答とmu=0対照を再計算する。
応答入力、出射ベクトル、EV/HLA/VLA、回転、エネルギー、衝撃量、状態・適用除外・checksを照合する。

Rust/Python数値差は1e-8、物理恒等式と法線対照は1e-7を許容差とする。
Python側は対象接触ごとに既存応答診断の2回と保存control照合用の1回、計3回の有限半径計算を行い、metadataへ記録する。
矛盾したselectedや回転、欠落／重複した投球キー、旧記録の偽補完は認めない。
出力先は新しいフォルダを指定し、DBも既存出力も上書きしない。

最初は先頭が`rust_collision_response_`の次の4ファイルを共有する。

- `summary.csv`
- `coverage.csv`
- `checks.csv`
- `metadata.json`

pairsとdetailsも出力するが、個別追跡や比較時に使う。
coverageのmatched_states／matched_plansは状態・適用状態を含めて照合した件数、
matched_responsesは実際に応答成功となった件数。selected_plansは選択された意図の数。
旧記録はmissing_intent_collision_statesとし、対象がなければno_comparable_rust_collision_responsesになる。

## 一致確認後の独立seed評価

最初の少数試合は移植の一致確認用として区別する。
一致後、規則・モデルを固定したまま別seedで試合を生成し、そのseedとゲームID範囲を評価記録へ残す。
前回の26650～27009範囲や移植調整用の試合を独立holdoutと呼ばない。

独立範囲でもまず同じ監査で数値一致とcoverageを確認し、全体／予測高さ別の接触・応答成功・フェア方向、
下向き・高角度・ライナー窓と共通フェア集団を比較する。結果を見て同じ標本で狙いや係数を再調整しない。
元のbootstrapの限界（ゲーム間で繰り返す選手・チームの依存、個別区間、モデル内評価）も保持する。

## 共通ケースとテスト

Pythonで生成した`tests/fixtures/finite_collision_contract.json`をRustとPythonの両方で照合する。
48応答ケース、14回転ケース、7反発曲線点、8つの計画／戦略接続ケースを含む。
摩擦なし・微小摩擦・目標到達、半径0、軸慣性、世界座標回転、入射回転、エネルギー増加目標の除外、
非接近、端部、無効パラメータ、監督／打者の不一致、時計不成立と無効回転を確認する。
Rust固有の非有限入力、新旧JSON互換、既存状態不変も検証する。

```sh
cargo test --locked --lib collision
cargo test --locked --lib contact_state
python3 -m unittest discover -s tests/diagnostics -p 'test_rust_collision_response_audit.py'
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
```

共通ケースを再生成する場合は変更内容をレビューする。生成器は既存の合成テスト状態も再利用するため、testsフォルダを含むcheckoutで実行する。

```sh
python3 tools/diagnostics/finite_collision_contract.py --output tests/fixtures/finite_collision_contract.json
```
