# 走塁ロジック

ソース: `src/domain/resolver/running_resolver.rs`

このリゾルバは、守備側のプレイ所要時間と走者の計算上の移動時間を比較して走者の進塁を決定します。`judge()` は `defense_time - runner_time >= 0.0` の場合に `Safe` を返し、それ以外の場合は `Out` を返します。ほとんどの打球・タッグアップ経路は `RunnersUnsaved` を返し、呼び出し側が後で `commit_unsaved_runners()` で新しい塁上の状態を確定できるようにします。

## 定数

- `ACCELERATION_LAG_TO_FIRST_BASE`: 打者走者の一塁到達までの追加加速時間。
- `ACCELERATION_LAG_AFTER_FIRST_BASE`: 一塁以降の各塁での追加加速時間。
- `ACCELERATION_LAG_FROM_FIRST_TO_SECOND_BASE`: 一塁から二塁へのラグ定数。現在は宣言のみで直接は使用されていません。
- `ACCELERATION_LAG_FROM_FIRST_TO_THIRD_BASE`: 一塁から三塁へのラグ定数。現在は宣言のみで直接は使用されていません。

## データ型

- `RunningEvent`: タッグアップ、盗塁、併殺、ゴロプレイ用の表示可能なイベントラベル。
- `StealRunnerAdvanceResult`: 盗塁試行の結果ペイロード。
- `DoublePlayRunnerAdvanceResult`: 併殺の第2送球の結果ペイロード。
- `RunnerAdvanceResult`: 打球時の走者進塁の共通結果ペイロード。
- `RunningPlan`: 外野安打後の打者走者と既存走者の目標塁。
- `RunnersUnsaved`: `RunnersOnBase` に確定される前の仮の塁上状態。
- `HitAttemptResult`: 外野安打の送球試行を解決するための内部結果。
- `RunnersOnBase`: 現在の打者走者と塁上の走者の状態。

## 関数一覧とロジック概要

### `RunningEvent::fmt()`

`t!()` を通じて `RunningEvent` をローカライズされた表示ラベルに変換します。

### `RunningPlan::set(defence_time, batter_to_second_time, batter_to_third_time)`

守備側の遅延から期待される外野安打のプランを選択します:

- 守備時間が打者の三塁到達時間より長い場合は三塁打プラン。
- そうでなく、守備時間が打者の二塁到達時間より長い場合は二塁打プラン。
- それ以外は単打プラン。

既存の走者はその安打規模に応じて楽観的に進塁させます。ランニング本塁打や走塁戦略のバリエーションはここではモデル化されていません。

### `RunnersUnsaved::put(base, runner)`

一塁・二塁・三塁に走者を仮配置します。本塁は得点として別途記録されるため無視されます。

### `RunnersUnsaved::put_if_some(base, runner)`

オプションの走者が `Some` の場合のみ仮配置します。本塁は無視されます。

### `RunnersUnsaved::score_if_some(runner)`

走者が存在する場合は `1`、それ以外は `0` を返します。得点した走者のカウントに使用されます。

### `RunnersUnsaved::runner_id_on(base)`

一塁・二塁・三塁に仮配置された走者IDを返します。空塁と本塁では `None` を返します。

### `advance_count(from, to)`

有効な塁間経路を進塁数に変換します。未対応の経路、逆進、同一塁への移動、打者走者の本塁から本塁への移動は `GameError::UnsupportedPath` で拒否します。

### `judge(defense_time, runner_time)`

`time_difference = defense_time - runner_time` を計算します。差が0以上なら安全（セーフ）、守備が先に到達していればアウトです。

### `RunnersOnBase::empty()`

打者走者とすべての占有塁をクリアします。

### `RunnersOnBase::has_runner_on(base)`

一塁・二塁・三塁が占有されているかを確認します。本塁は常に `false` を返します。

### `RunnersOnBase::is_loaded()`

一塁・二塁・三塁がすべて占有されている場合に `true` を返します。

### `RunnersOnBase::has_first_and_second()`

一塁と二塁が占有されている場合に `true` を返します。

### `RunnersOnBase::has_second_and_third()`

二塁と三塁が占有されている場合に `true` を返します。

### `RunnersOnBase::has_first_and_third()`

一塁と三塁が占有されている場合に `true` を返します。

### `RunnersOnBase::current_runners()`

現在の一塁・二塁・三塁の走者を `RunnersUnsaved` の仮配置オブジェクトにコピーします。

### `RunnersOnBase::runner_on(base)`

指定された出発塁の走者を取得します:

- 本塁は打者走者を意味します。
- 一塁・二塁・三塁は既存の塁上走者を意味します。
- 走者が存在しない場合は対応する `GameError` を返します。

### `RunnersOnBase::batter_runner_time_to(to_base, with_lag, batting_side)`

打者走者の本塁から目標塁までの移動時間を計算します:

- 距離には `advance_count(Home, to_base)` を使用します。
- 右打者には距離ペナルティとして 2.0 を加算します。
- `with_lag` が `true` の場合、加速ラグを加算します。
- 合計距離を打者走者の走力で割ります。

### `RunnersOnBase::runner_advance_time(runner, base_count, with_lag)`

既存の走者の移動時間を計算します:

- 距離は `BASE_DISTANCE * base_count` です。
- 経路からリード距離を差し引きます。
- 任意の加速ラグを各塁ごとに加算します。
- 最終的な時間は、調整後の距離を走者の走力で割った値です。

### `RunnersOnBase::steal_base_runner_time(from_base, to_base)`

`total_runner_time()` に走者の `start_reaction` を加算して盗塁時間を計算します。

### `RunnersOnBase::total_runner_time(from_base, to_base)`

既存の走者の通常進塁時間を計算します。同一塁を目標とする場合と本塁からの打者走者の移動を拒否し、出発塁の走者を取得して、経路を進塁数に変換した上で、ラグを有効にして `runner_advance_time()` を呼び出します。

### `RunnersOnBase::after_homerun()`

打者とすべての占有塁の走者に得点を記録し、直ちにすべての走者をクリアして得点数を返します。状態を直接変更するため `commit_unsaved_runners()` は不要です。

### `RunnersOnBase::commit_unsaved_runners(unsaved_runners)`

現在の一塁・二塁・三塁の状態を仮配置された走者で置き換えます。打者走者フィールドはここでは変更されません。

### `RunnersOnBase::after_infield_grounder(defense_play_result, batting_side)`

送球先に基づいて内野ゴロ後の走者の動きを解決します:

- 本塁送球: 打者を一塁へ、一塁走者を二塁へ、二塁走者を三塁へ進め、三塁走者がいる場合は本塁での判定を行います。
- 三塁送球: 打者を一塁へ、一塁走者を二塁へ進め、二塁走者がいる場合は三塁での判定を行い、三塁走者には得点を記録します。
- 二塁送球: 打者を一塁へ進め、一塁走者がいる場合は二塁での判定を行い、二塁走者を三塁へ進め、三塁走者には得点を記録します。
- 一塁送球: 打者走者を一塁で判定し、セーフなら単打を記録し、一塁走者を二塁へ、二塁走者を三塁へ進め、三塁走者には得点を記録します。

結果には、対象走者、判定、打撃結果、得点数、仮配置された走者が含まれます。

### `RunnersOnBase::resolve_triple_attempt(throw_target_base, defense_time, batting_side)`

打者が三塁を狙う外野安打プランの内部ヘルパー:

- 本塁送球は、得点を狙う一塁走者がいる場合にその走者を対象とします。
- 三塁送球は三塁を狙う打者走者を対象とします。
- その他の送球先では競走は発生しません。

打者走者が三塁でアウトになった場合、打撃結果は二塁打に格下げされます。それ以外は三塁打のままです。

### `RunnersOnBase::resolve_double_attempt(throw_target_base, defense_time, batting_side)`

打者が二塁を狙う外野安打プランの内部ヘルパー:

- 本塁送球は、まず二塁走者がいる場合はその走者を判定し、その本塁試行がセーフだった場合は次に一塁走者を判定します。
- 三塁送球は一塁走者を対象とします。
- 二塁送球は打者走者を対象とします。

打者走者が二塁でアウトになった場合、打撃結果は単打に格下げされます。それ以外は二塁打のままです。

### `RunnersOnBase::resolve_single_attempt(throw_target_base, defense_time, batting_side)`

打者が一塁を狙う外野安打プランの内部ヘルパー:

- 本塁送球は三塁走者を対象とします。
- 三塁送球は二塁走者を対象とします。
- 二塁送球は一塁走者を対象とします。
- 一塁送球は打者走者を対象とします。

打者走者が一塁でアウトになった場合、打撃結果はアウトになります。それ以外は単打のままです。

### `RunnersOnBase::score_for_existing_runners(batter_target_base, from_base, ruling)`

外野安打試行後に既存の走者の得点を数えます:

- 三塁打プラン: 三塁走者と二塁走者は常に得点し、一塁走者は競争プレイがセーフの場合のみ得点します。
- 二塁打プラン: 三塁走者は常に得点し、二塁走者と一塁走者は競争プレイがセーフの場合に得点します。一塁走者が封殺されるケースは特別に処理されます。
- 単打プラン: 本塁送球で三塁走者がアウトにならない限り、三塁走者は得点します。

ランニング本塁打の経路は拒否されます。

### `RunnersOnBase::build_runner_advance_result(batter_target_base, from_base, ruling)`

外野安打後の仮の塁上状態を構築します:

- 三塁打プラン: 打者はセーフの場合のみ三塁に仮配置されます。
- 二塁打プラン: 打者はセーフの場合に二塁へ仮配置されます。二塁走者が本塁でアウトになっても、打者は二塁に到達し、一塁走者は三塁に到達します。
- 単打プラン: 封殺された走者以外の既存走者は進塁し、打者は一塁でアウトにならない限り一塁に到達します。

得点した走者は仮配置から除外されます。

### `RunnersOnBase::after_outfield_hit(defense_play_result, batting_side)`

外野安打を最初から最後まで解決します:

1. ラグなしの打者の二塁・三塁到達時間を計算します。
2. 守備時間から `RunningPlan` を選択します。
3. 対応する単打・二塁打・三塁打の試行リゾルバを使用します。
4. 得点した走者を数えます。
5. 仮の塁上状態を構築します。
6. `RunnerAdvanceResult` を返します。

ランニング本塁打は未対応です。

### `RunnersOnBase::after_tagup(defense_play_result)`

タッグアップの進塁を解決します:

- 本塁送球は得点を狙う三塁走者を判定します。一塁・二塁走者は留まります。
- 三塁送球は進塁する二塁走者を判定します。一塁走者は留まり、三塁走者は得点します。
- その他の送球は一塁・二塁走者をそのままにして三塁走者に得点を記録します。

打球が捕球されているため、打撃結果はアウトのままです。

### `RunnersOnBase::after_double_play(double_play_defense_play_result, previous_runner_advanced_result, batting_side)`

最初のプレイで仮配置された走者を使って併殺の第2送球を解決します:

- 一塁送球は打者走者を対象とします。
- 二塁送球は一塁走者を対象とします。
- 三塁送球は二塁走者を対象とします。

守備側が第2の競走に勝った場合、対象の仮配置走者は除去され、打撃結果は `DoublePlay` になります。本塁送球は拒否されます。

### `RunnersOnBase::after_base_stealing(steal_defense_play_result)`

盗塁試行を解決し、塁上の状態を即座に変更します:

- 三塁送球は三塁へ盗塁する二塁走者を対象とします。
- 二塁送球は二塁へ盗塁する一塁走者を対象とします。
- その他の送球先は拒否されます。

セーフ判定の場合、走者は目標塁へ移動し、出発塁から除去されます。本塁盗塁と二重盗塁は未対応です。

## テスト用ヘルパー関数

テストモジュールはヘルパーコンストラクタも定義しています: `assert_near()`、`runner()`、`runner_with_lead()`、`runners()`、`defense_result()`、`double_play_result()`、`steal_result()`、`runner_advance_result()`。これらは簡潔なユニットテストのフィクスチャを構築するためだけに存在し、実行時のリゾルバロジックには含まれません。
