# 診断用DBパスの環境変数

ターミナルで一度設定すると、各Python診断コマンドでDBパスを省略できる。

```sh
export SABERBB_DB="/実際の保存先/saberbb.db"
```

macOSのzshで新しいターミナルにも引き継ぐ場合は、このexport行を `~/.zshrc` に追加して `source ~/.zshrc` を実行する。設定するのはSQLiteファイルのパスで、SQLite接続URLではない。空白を含むパスは引用符で囲む。`~/...` も展開する。相対パスはコマンド実行時の作業ディレクトリ基準なので、絶対パスを推奨する。

優先順位は明示した `--db` / `--pitch-db` → 環境変数 `SABERBB_DB`。未設定・空文字・空白だけの場合は未設定扱い。必須DB診断では従来どおり引数不足で終了する。環境変数がある場合、必須DB診断の `--db` は省略可能。

対応: `contact_collision_probe`、`contact_state_probe`、`rotating_bat_probe`、`execution_frame_probe`、`swing_prediction_probe`、`observation_update_probe`、`symmetric_height_probe`、`stance_axis_probe`、`bat_pivot_orientation_probe`、`finite_barrel_probe`。

```sh
python3 tools/diagnostics/bat_pivot_orientation_probe.py \
  --input-dir ../saberbb_observation_update_new \
  --output-dir ../saberbb_pivot_orientation \
  --variant observed_sigma_0.005_r0.9 \
  --gyro-sign 0
```

`finite_barrel_probe` では `--pitch-db` を省略するとSABERBB_DBから投球回転を読む。明示した `--ball-spin-rad-s X Y Z` は環境変数より優先するため、環境変数があってもゼロ回転や固定回転の対照実験を実行できる。明示的な `--pitch-db` と `--ball-spin-rad-s` の同時指定は従来どおりエラー。

環境変数も両引数もない場合、有限半径診断は従来どおり固定ゼロ回転。これに対し、SABERBB_DBを設定すると省略時はDBの投球回転を使う点に注意する。メタデータに実際の回転出典を記録する。

この設定はCLIのデフォルト値だけに適用する。Pythonから直接run()を呼ぶ場合はDBパスを引数で渡す。`.env` ファイルの自動読込は行わない。Rust本体のDB設定には影響しない。読み取り専用の接続方式は各診断で維持する。
