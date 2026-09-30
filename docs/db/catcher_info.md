# catcher_info

Stores catcher-specific attributes separately from shared fielding attributes,
following the `pitcher_info` model. Shared catcher fielding attributes remain in
`fielder_info` with `fielder_type = 'Catcher'`.

| Column | Type | Description |
| --- | --- | --- |
| player_id | INTEGER PRIMARY KEY | Player identifier |
| calling_style | TEXT NOT NULL | Balanced, Aggressive, Cautious, or Adaptive |
