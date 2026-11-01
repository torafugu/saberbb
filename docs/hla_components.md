# HLA生成項とMarginal再抽選の診断

75%のタイミング位置ずれ設定を維持したまま、HLAの要因を調べる。
今回は接触・EV・VLA・HLA・ファウル判定の計算や乱数消費は変えない。
Marginal専用タイミング再抽選の置換方式は、この比較後に決める。

既存DBへ `migrations/upgrades/add_hla_handedness_sign.sql` を1回適用し、
再ビルドして新しい試合を生成する。保存するのは打球生成時の実際の左右変換符号。
旧行はNULL。現在の選手情報から過去の打席の左右を推測して補完しない。

`sql/diagnostics/hla_components.sql` に:first_game_id / :last_game_idを指定する。
4本すべて独立して実行できる。SQLiteのASIN/ACOSを使う。

1. 接触記録数・比較可能数・保存HLAの再現性。hla_mismatchesは0を期待する。
2. 接触種別・左右符号別に、狙いのばらつき、pull、タイミング、反発の各寄与を比較する。
   平均の符号が相殺されないよう絶対値平均も出す。反発角のクランプ件数も表示する。
3. 現行HLAと、元の時間差へ戻したHLAの分布。元の狙い・pull・接触位置は固定する。
4. Marginalだけのフェア／ファウル方向の遷移件数。

現在の式の寄与は、左右符号hとして次の通り。

- approach = h * 0.85 * approach_variation_deg
- pull = h * 0.85 * pull_bias_deg
- timing = h * 0.85 * timing_angle_error
- rebound = h * 0.25 * degrees(asin(clamp(contact_offset_x_m / 0.070, -1, 1)))

4項の和をfinal_hla_degと独立比較する。反発を残差から逆算しない。
元の時間差を使う比較ではtimingだけを
`h * 0.85 * angular_velocity * adapted_timing_offset_sec * 180/pi` へ置き換える。
Marginalでは再抽選を取り除いた場合の方向差になる。Solid/Weakでは原則差がない。
保存済みの乱数を再利用するため、新しい抽選はしない。

abs(HLA)>=45は**打ち出し時点のファウル方向**。実際の飛行・スピン・最終位置や
ファウルフライ捕球まで再計算しないので、result='Foul'とは一致しない場合がある。
元タイミング版の方向から実際のファウル数・K率・安打数を直接推定しない。
HLAバンドは表示用の半開区間で、正負45度ちょうどの向きは2本目のabs判定で確認する。

missing_inputsがあれば旧記録混入などを確認する。同じ比較可能な接触群を両条件に使う。
まず1〜3本目、必要なら4本目を確認し、元の時間差を保ちつつ接触品質による方向の
不確実性をどこへ加えるかを検討する。今回は不確実性の強さを先に決めない。
