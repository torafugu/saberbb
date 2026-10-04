# 投手・捕手の配球候補の統合

投手・捕手はそれぞれの `PitchingPreferences` から短縮候補を作る。
`reconcile_pitch_call_proposals` は双方の候補の和集合を作り、各配球を双方の preferences で再評価して最終案を決定する。
候補から落ちたことを、投げられない・拒否したという意味には扱わない。

## 共通の評価 context

`PitchEvaluationContext::new(pitcher, batter, strategy, previous_call)` は、投手・打者・共通の Strategy・直前の投球を固定した評価 context を作る。
球種ごとの estimates、球速、usage による重みを作成時に保持する。
同じ context を候補生成と交渉に渡すことで、これらの計算を再利用できる。
投手・打者・Strategy・直前の投球が変わった場合は、新しい context を作る。

```rust
let context = PitchEvaluationContext::new(
    &pitcher, &batter, strategy, previous_call,
);
let pitcher_proposals = context.pitcher_proposals(&pitcher_preferences, 3, 2);
let catcher_proposals = context.catcher_proposals(&catcher_preferences, 4);
let decision = reconcile_pitch_call_proposals(
    &pitcher_proposals,
    &catcher_proposals,
    &pitcher_preferences,
    &catcher_preferences,
    &context,
    0.5,
);
```

`select_pitch_call` は上の双方の候補・preferences・context を受け取り、重み 0.5 で `Option<PitchCall>` を返す。
既存の `pitcher_pitch_call_proposals` と `catcher_pitch_call_proposals` は引数を維持したラッパーとして残る。
計算を再利用する場合は context のメソッドを使う。

## 単一候補の採点

`evaluate_pitch_call(call, preferences, context)` は `Option<PitchCallProposal>` を返す。
候補生成と交渉はどちらもこの関数を使い、次の共通項と個別の希望を評価する。

- 共通項：usage、球種・コースの Strategy 評価、打者との相性、四球・強打・実行リスク。
- 個別項：コース・Margin・球種構成・配球順序・リスク回避の希望。

投手の持ち球にない球種、非有限の評価値やリスクは `None` とする。
希望に合わないだけの配球は除外せず、スコアで評価する。
球種の短縮選択で使う概算値は、最終評価へ重複加算しない。

## 交渉と決定

1. 球種・TargetZone・Margin が一致する重複候補を除き、和集合を作る。順序は投手候補、その後に捕手独自の候補。
2. 和集合に含まれる各候補を双方で採点し直す。持ち球にない候補と、双方の有効な評価が得られない候補は除外する。
3. `(1 - catcher_weight) * pitcher_score + catcher_weight * catcher_score` の最大値を採用する。
4. 同点なら投手の評価が高い候補、さらに同点なら和集合の入力順を優先する。
5. 有効な候補がない場合は `None` とする。

提案時のスコアやリスクは再利用せず、現在の context を採点の根拠とする。
古いスコアや候補の重複が選択を歪めないようにする。
和集合が N 件なら最大 2N 件の採点を行い、交渉時に全球種×全コースを探索し直すことはない。
片側のリストが空でも、他方の候補を双方で評価して決める。

`catcher_weight` は 0〜1 に制限し、非有限値は 0.5 とする。
0 は和集合の中から投手評価だけで、1 は捕手評価だけで選ぶ。
共通項を二重加算しないよう、双方のスコアは平均で統合する。

## 判断の記録

`PitchCallDecision` は配球、統合スコア、双方のスコア、次の理由を保持する。

- `Agreement`：当初から双方にあった候補を採用。
- `ReevaluatedAgreement`：当初は片側だけにあった候補を、双方の再評価後に採用。
- `PitcherChoice` / `CatcherChoice`：重み 0 / 1 で選択。

正常な選択結果では双方のスコアが `Some` になる。
preferences の変更や再提案を繰り返す処理は行わず、一回の再評価で決める。
ゲーム内の投球実行への接続は別の処理とする。
