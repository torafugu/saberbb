# バット軸の傾きと法線の左右成分

かすり気味の接触で前方速度が不足する条件を、姿勢別に確認する。
保存された姿勢・法線・衝突応答だけを使い、モデルや係数、接触探索を変更しない。

## 傾きと座標

ワールド+Yを前方、Xを横、Zを上とする。
各接触で `tilt = atan2(abs(axis_z), hypot(axis_x,axis_y))` を計算する。
範囲は0〜90度で、軸ベクトルを反転しても同じ値になる。
符号付きのノブ→先端の向きや、打者左右によるミラー正規化は含まない。
平均axis_zから傾きを逆算せず、各接触の傾きを集計する。

傾き区間は10/20/30/40/50/60/75/90度が境界。
90度は最後の90+区間に入るが、幾何の定義上90度より大きな傾きはない。
下限を含み上限を含まない。

## 法線の分解

単位バット軸aと法線nについて、側面接触ではa・n=0を検証する。
ワールド前方f=(0,1,0)を軸に直交する面へ投影し、
`ef = normalize(f - (f・a)a)`、`ec = a × ef` とする。

`n = (n・ef)ef + (n・ec)ec`

この分解の左右成分をforward_normal_x、circumferential_normal_xとして出力する。
その和がnormal_xに一致することも含め、3次元の再構成を検査する。
軸を反転するとecと係数の両方が反転し、寄与ベクトル自体は変わらない。
これは法線の幾何学的分解であり、前方/周方向の力積や因果寄与の分解ではない。
姿勢と方向の関連は確認できるが、それだけで法線生成処理の不具合とは断定しない。

axisとnormalの単位長、側面の直交性、基底での法線閉合が1e-8を超えて不一致なら停止する。
端面接触はnot_sideとして残し、側面直交条件や側面用分解を強制しない。
軸が前方にほぼ平行で投影長≤1e-8の場合はforward_projection_undefined。
分解値をゼロに補完せず、n_*を0として記録する。

## 出力

- `finite_barrel_tilt_coverage.csv`: モデルに依存しない幾何接触数、側面/端面、分解定義可否。
- `finite_barrel_tilt_checks.csv`: 単位長・側面直交性・法線分解の最大残差（無次元）。
- `finite_barrel_tilt_geometry.csv`: 接触数、傾き、軸の|Y|成分、法線X/Y/Z、法線左右成分の絶対値、
  分解した左右寄与とその絶対値、打点位置、バット接触点速度の平均・有効数。
- `finite_barrel_tilt_outcomes.csv`: 同じ幾何区間ごとのsource/各モデルの成功/未評価・方向別件数、
  EV/VLA、|打球X速度|、打球Y速度、Y−|X|、弱い/高速/下向き/高角度/高速下向きの領域件数・割合。

stratumはoverall、tilt、tilt_impact（傾き×局所絶対b/R）、
 tilt_impact_normal_z（さらに法線z帯）の4種類。
段階ではなく同じ集合の別集計なので合算しない。scope・variant・モデルも重複する。
geometryはモデル別に複製せず、全幾何接触を対象とする。

outcomesでは未評価の件数を保持する。成功接触の分布を見るときは
collision_status=impulse_contactを選び、fair_directionの件数を成功接触数で割る。
低EVなどのfractionは方向別行内の有効値数n_*が分母で、フェア率ではない。
source_constant_mass_normalは保存済みの基準法線応答であり、各モデルは今回指定した回転での応答。
各モデルで有効集合が違うため、平均の直接差を同一接触の効果と解釈しない。

normal_*とaxis成分は無次元、角度は度、速度はm/s、source_z_mはノブからのm。
前方余裕Y−|X|は正ならフェア円錐内だが、落下後まで含めたフェア打球判定ではない。
全幾何接触も、観測更新CSVに含まれるスイングという上流選別は残る。

## 実行と読み方

同じ観測更新CSVとその元DBを指定する。試合の再生成は不要。
出力先は未作成ディレクトリにする。

```sh
python3 tools/diagnostics/finite_barrel_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_finite_tilt \
  --pitch-db "/path/to/saberbb.db" \
  --gyro-sign 0
```

1. coverageとchecksを確認する。
2. geometryのtiltで傾き分布を確認し、モデル適用範囲の偏りをoutcomesの未評価件数と照合する。
3. tilt_impactでb/Rの帯を固定し、傾きごとのフェア率・高角度打球・|normal_x|を比較する。
4. tilt_impact_normal_zで上下成分も固定し、左右成分の関連が残るか確認する。
5. 軸の前方成分が混ざる可能性はavg_abs_axis_y、水平速度の変化は既存horizontal出力と併せて読む。

姿勢を変える反実仮想や実測バット姿勢との校正は行わない。
法線が正しく構成されていても、姿勢分布自体が実際と違う可能性は残る。
