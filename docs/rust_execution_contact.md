# 選択した打球意図を前向きの実行・接触診断へ接続する

観測更新後の予測から生成した2意図の開始姿勢をRustで前向きに実行し、有限長バットと球の幾何学的な接触を検証する。
監督・コーチの方針、打者の選好、最終選択を保存し、各候補の`selected`で対応を示す。
戦略階層と設定方法は[観測更新と打者戦略](observed_batting_strategy.md)を参照。

## 入力と因果関係

意図の判断は投球生成前に固定し、計画は過去の観測から更新した予測だけを使う。
実球の位置・速度・到達時刻は、その後の接触検証にだけ使う。
法線や接触結果から意図・狙い・開始姿勢を選び直さない。

`execution_pose()`は開始姿勢からsin²速度パルスを積分する。
位相は「絶対時刻 − 予測から決めた予定時刻」であり、実球の本塁面到達時刻に合わせてスイングを再配置しない。
検証の時刻座標を変換するためだけに、予定時刻と実球の基準時刻の差を使う。
既存の角度実行誤差を保持し、新しい遅延や能力誤差は加えない。

接触探索はPythonの`rotating_bat_probe.search()`と同じ保守的な距離ステップを用いる。
速度上界は球の速度、支点の最大並進速度、最大角速度×（lever＋半長）の和。
探索設定は本塁面基準の±0.06秒、有効半径0.07 m、許容距離1e-6 m、最大4000反復。
球はこの短区間で等速直線運動とし、バットの有効部分は半長0.16 mの線分と丸い端部で近似する。
衝突応答、EV、VLA、回転数の計算は今回の範囲に含まない。ライブの接触・打球結果・乱数は変更しない。

## 保存する内容

`player_game_contact_state.state_json.execution_contact`を任意フィールドとして追加する。DB移行は不要。

| フィールド | 内容 |
| --- | --- |
| version / source | 1 / intent_execution_contact_shadow_v1 |
| decision | 監督指示、打者選好、選択意図、判断理由 |
| settings | 探索区間・有効半径・許容距離・反復上限 |
| probes | 2意図それぞれのselected、無効理由、軌道サンプル、探索結果 |
| samples | 予定時刻の−0.03／0／＋0.03秒における基準点、軸、速度、角速度 |
| search | 状態、反復数、評価点の最小clearance、接触時刻・接触幾何 |

`contact`の場合だけ接触時刻、法線、軸方向位置、接触点のバット速度、side／capを保存する。
評価点の最小clearanceは区間全体の最小値を保証しない。

| 状態 | 扱い |
| --- | --- |
| contact | 許容距離内への進入を探索で確認 |
| no_contact | 探索区間内に接触なし |
| unresolved | 反復上限または刻みが進まないため未確定 |
| overlap_at_start | 探索開始時点ですでに重なり・許容距離内。新たな衝突として数えない |
| execution_starts_before_planなど | 元の時計不成立理由を保持し、実行・探索しない |
| invalid_contact_search_input | 探索入力が不成立。接触なしと混同しない |

観測更新が不成立なら`execution_contact`も生成しない。priorの計画へは戻さない。
時計不成立はsamplesが空、searchがnullとなる。選択された候補が不成立でももう片方へ自動で切り替えない。

## DBからの照合

この変更を取り込んだRustで新しい試合を生成・保存する。古い行の新フィールドは補完しない。
DBパスは環境変数`SABERBB_DB`に設定しておく。新しい監査CLIは`--db`を受け取らず、この変数から取得する。
以下のIDは例であり、実際に生成した範囲に置き換える。まず10試合程度で実行時間とcoverageを確認する。

```sh
python3 tools/diagnostics/rust_execution_contact_audit.py --first-game-id 26587 --last-game-id 26596 --output-dir ../saberbb_rust_execution_contact_audit
```

読み取り専用トランザクションで標準ノイズ・観測更新・両意図の計画を再生成して照合し、
同じ開始姿勢からPythonでも軌道と接触探索を再実行する。数値差1e-8超、選択の不一致、状態・無効理由の不一致はエラー。
DBと既存の出力フォルダは上書きしない。新たな接触探索は行うが、衝突応答は再計算しない。

共有するファイルは次の4点。

- `rust_execution_contact_summary.csv`
- `rust_execution_contact_coverage.csv`
- `rust_execution_contact_checks.csv`
- `rust_execution_contact_metadata.json`

details CSVは投球キーと個別状態を調べるときに使う。
summaryは両候補の`all_plans`と選択された候補の`selected_plans`を分け、意図・監督指示・打者選好別に集計する。
両scopeは重複し、同じ投球の2候補も独立試行ではない。
`contact_pct_of_searched`は未収束と開始時重なりを含む全探索が分母、`contact_pct_of_resolved`はcontact＋no_contactのみが分母。
不成立・未確定の件数を併せて読み、ライブのContact%と同一視しない。
下向き率・高角度率の比較は、接触幾何の一致確認後に衝突応答を接続して行う。Statcastの構成比調整とは分ける。

## 検証

Python生成の共通20ケースで左右打者、両意図、速度比・角度誤差、側面・端部接触、接触なし、開始時重なり、未収束を照合する。
新旧JSON互換、既存状態の不変性、監督指示と打者選好の不一致、時計不成立時の非フォールバック、
保存値の改変拒否、DB読み取り専用、環境変数によるCLI実行も検証する。

```sh
cargo test --locked --lib intent_execution_contact
cargo test --locked --lib contact_state
python3 -m unittest discover -s tests/diagnostics -p 'test_rust_execution_contact_audit.py'
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
```

共通ケースを再生成する場合は変更内容をレビューする。

```sh
python3 tools/diagnostics/intent_execution_contract.py --output tests/fixtures/intent_execution_contract.json
```
