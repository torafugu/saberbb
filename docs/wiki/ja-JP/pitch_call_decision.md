# 投手・捕手の配球候補の統合

`PitchingStrategy` は共通の上位方針とし、投手と捕手がそれぞれ
`PitchingPreferences` から生成した `PitchCallProposal` を照合する。

## API

- `select_pitch_call(pitcher_proposals, catcher_proposals)` は双方を等しい重みで評価し、`Option<PitchCall>` を返す。
- `reconcile_pitch_call_proposals(pitcher_proposals, catcher_proposals, catcher_weight)` は重みを指定し、選択の根拠を含む `Option<PitchCallDecision>` を返す。

```rust
let decision = reconcile_pitch_call_proposals(
    &pitcher_proposals,
    &catcher_proposals,
    0.5,
);
if let Some(decision) = decision {
    let pitch_call = decision.pitch_call;
    // decision.reason / pitcher_score / catcher_score で判断を記録できる。
}
```

## 初期ルール

1. 球種・TargetZone・Margin がすべて一致する候補を共通候補とする。
2. 共通候補の評価値を `(1 - catcher_weight) * pitcher_score + catcher_weight * catcher_score` とする。
3. 最大評価値の候補を採用する。同点なら投手側の評価が高いものを優先し、さらに同点なら投手候補の入力順を維持する。
4. 共通候補がなければ投手側の最良候補を採用する。投手候補がなければ捕手側の最良候補を採用する。
5. 有効な候補が双方にない場合は `None` とする。

双方のスコアには共通の Strategy・能力・リスク評価が含まれるため、
単純加算ではなく重み付き平均を使う。ここでリスクの再加算はしない。
スコアは確率ではなく相対的な評価値であり、負の値も有効。

`catcher_weight` の初期値は 0.5。0 は投手、1 は捕手のみで選び、指定側が空なら他方へフォールバックする。
有限値は 0〜1 に制限し、NaN・無限大は 0.5 として扱う。
非有限の候補スコアは除外し、同じ候補が重複していればその側の最高スコアを使う。

## 候補数と選択範囲

この処理は、渡された短縮候補を照合する段階であり、新しい球種やコースは生成しない。
候補にない配球のスコアを 0 とみなすこともしない。
共通候補が一つでもあれば、その中から選ぶため、片側だけが提案した候補は選ばれない。
捕手独自の提案まで交渉対象にしたい場合は、双方にその候補を再評価させる段階が別途必要。

選択結果の `reason` は `Agreement`、`PitcherChoice`、`CatcherChoice`。
片側の候補しか使わなかった場合、他方のスコアは存在すれば記録し、なければ `None` とする。

候補生成から決定までの関数を追加した段階であり、ゲーム内の投球実行への接続は別の処理とする。
