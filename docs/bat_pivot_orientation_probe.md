# バットと支点の上下関係を変える対照実験

`bat_axis_origin_audit` で、支点が地面より低い試行が広く確認されたため、姿勢の仮定に対する感度を調べる。現行結果 `saved_up` と、軸の上下関係を変える `matched_down` を同じ投球で比較する。モデルの校正やゲーム本体の修正ではない。

```bash
python3 tools/diagnostics/bat_pivot_orientation_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_pivot_orientation \
  --variant observed_sigma_0.005_r0.9 \
  --gyro-sign 0
```

入力ディレクトリは観測更新診断の保存先で、`observation_update_inputs.jsonl`、`observation_update_contacts.csv`、`observation_update_metadata.json`、`observation_update_coverage.csv` が必要。DBはその入力を作ったときの `player_game_contact_state` と `player_game_pitching` を含むものを指定する。選手再生成・試合再実行は不要。既存の出力ディレクトリへの上書きは拒否する。

DBパスは事前に `export SABERBB_DB="/実際の保存先/saberbb.db"` で設定する。明示した `--db` も使用可能。詳細は[診断用DB設定](diagnostic_database.md)。

## 固定するものと変更するもの

共通の予測打点と基準時刻を用い、実行角誤差を加えた保存基準点の**位置と世界速度を両条件で一致**させる。この実行基準点は実行誤差によって予測打点からずれることがある。そのずれも共通のまま保持する。

保存軸 `a=(h*cos(theta),0,sin(theta))`、保存角速度 `omega`、基準点位置P・速度V、回転半径Lに対して、Y軸まわりに `2*h*theta` 回す回転Qを用いる。

- 新軸 `a'=Q*a=(h*cos(theta),0,-sin(theta))`
- 新角速度 `omega'=Q*omega`（大きさを保持）
- 新支点 `pivot'=P-L*a'`
- 新支点速度 `vp'=V-cross(omega',L*a')`

これにより基準点の位置Pと速度Vを保ち、バットの基準点を支点より低く置く。左右打者で回転符号を変える。thetaが0であれば元の軌道に一致する。符号付きthetaは実行誤差を加えた保存軸から求め、接触時のunsigned tiltからは求めない。

支点の位置・並進速度、世界座標での角速度の向き、接触までの曲率が変わる。これはバットと支点を含む運動仮説の比較であり、地面以下の支点だけを切り上げる補正ではない。支点を手元の実測値として扱わず、身体関節の制約も新たに仮定しない。

## 計算範囲と入力検査

- 既存の `bat_axis_origin_audit` を先に実行し、全条件の保存姿勢・キー対応・件数・重複・条件完全性を検査。結果と入力ハッシュは `source_audit/` に出力する。
- 指定した1つのvariantを処理し、DBの状態・左右・予測位置との差・到達時刻との差を照合する。既存接触例では入力球速ベクトルも照合する。DB全体の同一性や非接触例の元球速の独立再現までは保証しない。
- 現行条件は保存済み接触結果を再利用。変更条件のみ1試行1回の接触探索を行う。探索窓は保存値、空間許容誤差1e-6m、反復上限4000を元の観測更新診断と揃える。初期重複や未解決も保持する。
- 投球回転は指定DBから全対象投球について取得し、同一の世界回転ベクトルを両条件に適用。欠落・重複・不正値はエラーとする。gyroの符号は `--gyro-sign -1/0/1` による仮説で、効率から推定しない。
- 応答モデルは `normal_profile`、対照 `finite_mu0`、主モデル `finite_I0.00015_et0.4` の3つに限定。従来と同じ有限半径・質量・位置別反発係数を使う。端部・反発曲線の範囲外などは未評価のまま保持する。
- ゼロ遅延対照、ゼロ回転対照、接地後軌道は計算しない。DBは読み取り専用。スクリプトはDBの更新・ゲームの乱数消費を行わない。

全入力の読み取り・ハッシュ・姿勢監査と、選択条件の新規接触探索には時間が必要。従来の全条件・全応答診断より計算対象を減らしているが、実データ・ユーザーのMacでの所要時間は未測定。

## 出力

| ファイル | 内容 |
| --- | --- |
| `bat_pivot_orientation_summary.csv` | 全体・計画傾き別の接触率、成功数、フェア方向率、EV/VLA、法線、横速度、支点高さ |
| `bat_pivot_orientation_pairs.csv` | 両条件で同じ応答モデルが成功した投球だけのEV/VLAとフェア方向率 |
| `bat_pivot_orientation_transitions.csv` | 非接触・未解決・未評価理由・フェア方向の条件間遷移 |
| `bat_pivot_orientation_ev_vla.csv` | 全成功接触とフェア方向それぞれのEV/VLA二次元ヒストグラム |
| `bat_pivot_orientation_checks.csv` | 入力照合、基準点位置/速度一致、応答の保存則・対照誤差 |
| `bat_pivot_orientation_coverage.csv` | 比較試行数と条件別の採用/未評価件数 |
| `bat_pivot_orientation_metadata.json` | 条件、モデル、投球回転の出典・仮定、新規探索回数 |

`summary` の `trials` は非接触・未解決・端部・曲線範囲外を含む全対象試行。`geometry_contacts` は端部を含む幾何接触、`successful_contacts` はその応答モデルが成功した接触。`contact_fraction` は前者/trials、`fair_fraction_of_successful` はフェア方向数/successful_contacts。フェア方向は衝突直後の水平角による判定で、飛行後のフェア/ファウル判定ではない。

`avg_` は対応する `n_` を分母とする。EV/VLAは成功例、法線・接触時傾き・点速度は幾何接触、支点高さは全試行で定義される。欠損を0として平均しない。`proxy_pivot_below_ground` は基準時刻の支点z<0の件数。`low_ev` はEV<25m/s、`high_ev` はEV>=45m/s、`downward` はVLA<0度、`high_vla` はVLA>=50度で、いずれも成功接触だけで数える。

`pairs` の `before_` はsaved_up、`after_` はmatched_down。分母は `paired_successful_contacts`。接触/非接触や採用/不採用が入れ替わった投球を除くため、全体比較と合わせて読む。`before_fair`/`after_fair` はこの共通集合での0〜1の割合。

ヒストグラムは非ゼロセルだけを出力。`cohort_contacts` が条件・モデル・scope・cohortごとの分母で、`fraction` はセル件数/分母。該当cohortが0件ならその行は出力せず、summaryの件数で確認する。`all_successful` と `fair_direction`、異なるscope・stratum・モデルは重なるため合算しない。

改善が出ても、姿勢の仮定への感度を示す結果であり、そのまま実際の打者姿勢の正解や採用すべき係数と断定しない。実データの結果を見てゲーム側への適用を判断する。
