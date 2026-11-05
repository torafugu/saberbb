# seedと生成条件を固定した打球意図のholdout評価

27010～27054の45試合でRust/Python応答の一致を確認した後、
次の検証用データは**生成前にseed・試合の選び方・除外範囲を指定**して作る。
新しいゲームIDだけを独立seedの証拠にはしない。

`intent_holdout`は通常のGameServiceと共通の試合ループを使う専用CLI。
既存の監督／打者の判断規則と両意図、予測・スイング・応答係数を変更しない。
通常の`cargo run -- --process ...`は従来の起動先と挙動を維持する。

## 生成前に決める条件

| 条件 | 初回の実行例 |
| --- | --- |
| master seed | 2026110501 |
| 対象 | 次の40予定ラウンドにある全未処理試合 |
| 除外する既知範囲 | game_id ≤ 27054 |
| 打者設定 | 両チームLetBatterDecide、選手選好は既定ContactFirst |
| 比較 | 同じ投球情報でContactFirst／LineDriveの両診断候補 |
| 応答 | finite_I0.00015_et0.4、既存の固定係数と狙い |
| 1試合の投球予算 | 10000。超過時はrun全体を失敗扱い |

`rounds`は試合数ではなくラウンド数。実際の試合数は日程・リーグ構成で変わる。
母集団の選択は「次の指定ラウンドの全未処理試合」で固定し、結果による除外やseed探索をしない。
各ラウンドの全試合入力を先に保存してから、そのラウンドの結果を生成する。

## 1. 明示seedで新規試合を生成する

```sh
cargo run --release --locked --bin intent_holdout -- run --seed 2026110501 --rounds 40 --exclude-through-game-id 27054 --output-dir ../saberbb_holdout_seed_2026110501
```

このコマンドは**通常の試合結果としてDBへ保存し、処理済みラウンドを進める**。
既存データの削除・初期化や日程生成はしない。予定試合がない場合や除外範囲に重なる場合は停止する。
Rust側のDBは従来のアプリ設定`database_path`を使う。同時に別の試合処理を実行しない。

ゲームごとのseedは`SHA256(namespace + u64le(master seed) + u32le(game_id))`の先頭8byteをu64leとして生成する。
namespaceは`b"saberbb:holdout:v1\0"`。単一の連続乱数列を試合数で分割する方式ではなく、各試合で`RealRng::from_seed`を初期化する。
seedはJSONでは文字列として記録し、u64の精度を保つ。

出力先は新しいフォルダを使う。同じフォルダの再利用や上書きは拒否する。
通常設定と同じスコア計算を使うが、診断経路では既存u8カウンターが溢れる前のスコア上限も設け、該当runを失敗扱いにする。
投球予算やスコア上限に達した試合を除いて、残りだけを成功したholdoutとして扱わない。

## 保存される生成証跡

| ファイル・項目 | 内容 |
| --- | --- |
| manifest.json | seed、選択条件、除外境界、予定／完了ラウンド、状態、試合一覧 |
| cases/game_ID.json | 生成前の日程、全選手能力、球場・ルール、打者設定、試合seed、投球予算 |
| engine_source_sha256 | ビルド時のsrc全Rustファイル、Cargo.toml／lock、build.rsのハッシュ |
| build情報 | コンパイラ、target、profile、その識別ハッシュ、Git commit／dirtyの記録 |
| input_sha256 | 保存したcaseの正確なファイル内容 |
| result_sha256 | 正規化JSONにした試合結果全体のハッシュ |
| contacts_sha256 | DBに保存する診断行の正確なpayload・外側の状態を束ねたハッシュ |
| frozen_policy_sha256 | 既存の凍結済み狙い規則のファイルハッシュ |

入力・生成条件は結果の生成前に保存する。最初の実行も、保存したcaseファイルを読み直した値から始める。
manifestの各試合はprepared→savedとなり、指定ラウンドがすべて完了した場合だけrunはcompleteになる。
失敗はfailed、強制終了などで最後まで更新できなかったものはrunningのまま残る。評価側はどちらも拒否する。

DB保存と外部manifestは一つのトランザクションではない。途中失敗時にはDBへ一部の試合が保存されている可能性がある。
自動削除・巻き戻し・同一runへの継ぎ足しはせず、部分runを評価母集団にしない。

## 2. 生成証跡とDB行を照合して応答を評価する

PythonのDBパスは引き続き環境変数`SABERBB_DB`から取得する。
生成先のRust設定と同じDBを指定しておく。

```sh
python3 tools/diagnostics/seeded_holdout_audit.py --run-dir ../saberbb_holdout_seed_2026110501 --output-dir ../saberbb_holdout_seed_2026110501_audit
```

manifestの完了状態、除外境界、各試合seedの導出、固定設定、caseファイルのハッシュを先に確認する。
その後、**manifestに記録された試合IDだけ**を対象として、同じ読み取り専用トランザクションで
DB診断行のハッシュとRust/Python応答を照合する。ID範囲の間に別runの試合があっても混ぜない。
空間・時刻・物理の照合に加え、DBのログが今回の生成証跡に結び付いていることを確認する。

生成中や未完了のrun、変更されたcase、欠落・変更されたDB診断行は拒否する。
元DBの場所が変わっても、記録した診断行と保存投球が一致すれば検証できる。
DBもrunフォルダも変更しない。監査の新しい出力フォルダに生成証跡を含むmetadataと通常の応答集計を保存する。

## 3. 同じ独立範囲でゲーム単位bootstrapを行う

```sh
python3 tools/diagnostics/rust_intent_game_bootstrap.py --input-dir ../saberbb_holdout_seed_2026110501_audit --output-dir ../saberbb_holdout_seed_2026110501_bootstrap --replicates 2000 --bootstrap-seed 0
```

既存のPython衝突キャッシュに加え、照合済みRust応答キャッシュも読める。
診断用の2候補を同一投球の対にし、ゲーム全体を共通の抽出回数で復元抽出する。
検証済みmanifestに記録された全ゲームを抽出母集団とし、比較投球が0だったゲームも寄与0として保持する。
分母、共通フェア集団、未定義反復、個別区間の扱いは[既存bootstrap](rust_intent_game_bootstrap.md)と同じ。
接触探索・応答の再計算は行わず、元の物理checksも新しいchecksへ引き継ぐ。

最後のフォルダにある次の4ファイルを共有する。

- `rust_intent_game_bootstrap_summary.csv`
- `rust_intent_game_bootstrap_coverage.csv`
- `rust_intent_game_bootstrap_checks.csv`
- `rust_intent_game_bootstrap_metadata.json`

metadataのsource_metadataへseed・生成証跡・応答のcoverageを含める。
必要時には監査フォルダの4ファイルやmanifestも個別追跡に使う。全選手を含むcaseファイルは通常の共有には不要。

## DBなしで一つの試合を再実行する

```sh
cargo run --release --locked --bin intent_holdout -- replay --run-dir ../saberbb_holdout_seed_2026110501 --game-id 27055
```

game-idはmanifestに実在するIDへ置き換える。
replayはアプリDBを開かず、保存caseだけで同じ試合を再実行し、試合結果全体と診断行のハッシュを照合する。
生成時と同じソース、コンパイラ、target、profileを使う。異なる識別ハッシュなら停止する。
記録したGit commitはcheckoutの手掛かりであり、dirtyビルドでは変更したソースの復元も必要になる。
このreplayは結果の再現確認であり、別の独立試合として件数を増やさない。

## 解釈の限界

過去の未指定seedの履歴は復元できない。今回のrunで、事前指定seedと凍結した入力から新規に生成したことを検証する。
同じ選手・チームの別試合まで統計的に独立になるわけではない。
既知データ・移植確認用45試合と今回のholdoutを区別し、結果を見て同じ標本で狙いや係数を変更しない。
診断候補の比較であり、ライブ打撃の結果に新しい戦略・応答を適用する変更ではない。

## 検証

```sh
cargo test --locked --lib seeded_diagnostic
cargo check --locked --bins
python3 -m unittest discover -s tests/diagnostics -p 'test_seeded_holdout_audit.py'
python3 -m unittest discover -s tests/diagnostics -p 'test_rust_intent_game_bootstrap.py'
python3 -m unittest discover -s tests/diagnostics -p 'test_*.py'
```

同一caseの試合結果・診断ログの再現、seed／行ハッシュの言語間契約、事前入力保存、
失敗時の非完了・ラウンド非進行、除外／重複／上書き拒否、DBとcaseの改変拒否、
manifestの正確な試合集合だけの評価、DB読み取り専用、既存／Rust応答キャッシュの同じbootstrap結果を確認する。
