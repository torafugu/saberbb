# SQL statistics reference

Summaries and output examples for all 28 queries in [`sql/`](sql/).
All queries read `player_game_batting`; `short_distance_triples.sql` also joins
`player_game_fielding` and `player_game_running`. They are read-only analyses
of the rows currently in the database, with no season or game restriction.

## Running a query

From the repository root, using a populated simulation database:

```sh
sqlite3 -header -csv /path/to/saberbb.db < stats/sql/batting_result_dist.sql
```

Examples below are excerpts (up to four data rows) from the latest saved CSV
by filename for each query in [`data/`](data/), not newly executed simulation
results. Each example links to its source. Different exports may represent
different simulation runs or settings, so totals need not agree across queries.
`short_distance_triples.sql` has no saved CSV; its example is explicitly illustrative.
Blank CSV fields represent NULL. The SQL files were also checked against an
empty database built from the current batting, fielding, and running DDL;
query-specific syntax/schema incompatibilities are noted below.

## Shared conventions

- **Batted results** means `Out`, `Single`, `Double`, `Triple`, `HomeRun`,
  `DoublePlay`, `FieldersChoice`, and `Foul` where that filter is used.
  Queries selecting non-NULL HLA alone can also include non-contact events.
- **HLA** is horizontal launch angle in degrees. The foul-zone diagnostics use
  `ABS(HLA) > 45`; exactly ±45 degrees is inside for that comparison.
- **Normalized tip offset** is `length_offset_m / 0.16`. Nonnegative offsets
  select the tip side, and 1.00 corresponds to 0.16 m. Interval bands generally
  include the lower bound and exclude the upper bound; empty bands are omitted.
- Speeds are m/s unless explicitly converted to km/h; distances are meters,
  angles are degrees, and times are seconds unless marked otherwise.
  Timing conversions and the raw-timing histogram exception are described below.
- `pct` values are percentages (0–100); `pp` changes are percentage points.
  `LAG` compares the previous populated band, not necessarily an adjacent interval.

## Query catalog

- [angle_p_analysis.sql](#angle_p_analysis)
- [avg_launch_speed.sql](#avg_launch_speed)
- [base_hla_final_hla.sql](#base_hla_final_hla)
- [batting_result_ball_spec_dist.sql](#batting_result_ball_spec_dist)
- [batting_result_dist.sql](#batting_result_dist)
- [change_from_previous_pp_1cm.sql](#change_from_previous_pp_1cm)
- [change_from_previous_pp_2cm.sql](#change_from_previous_pp_2cm)
- [contact_type_around_tip.sql](#contact_type_around_tip)
- [contact_type_around_tip_detail.sql](#contact_type_around_tip_detail)
- [contact_type_dist.sql](#contact_type_dist)
- [contact_type_transition.sql](#contact_type_transition)
- [fair_with_high_offset.sql](#fair_with_high_offset)
- [foul_all_dist.sql](#foul_all_dist)
- [foul_classification_dist.sql](#foul_classification_dist)
- [fouls_hla_foul_zone_dist.sql](#fouls_hla_foul_zone_dist)
- [hla_dist.sql](#hla_dist)
- [homerun_dist.sql](#homerun_dist)
- [invalid_contacts_beyond_tip.sql](#invalid_contacts_beyond_tip)
- [monotony.sql](#monotony)
- [normalized_tip_offset_contact.sql](#normalized_tip_offset_contact)
- [outbound_result_dist.sql](#outbound_result_dist)
- [seconds_milliseconds_check.sql](#seconds_milliseconds_check)
- [short_distance_triples.sql](#short_distance_triples)
- [signed_hla_dist.sql](#signed_hla_dist)
- [timing_band_analysis.sql](#timing_band_analysis)
- [timing_error_dist_dist.sql](#timing_error_dist_dist)
- [timing_error_unit_check.sql](#timing_error_unit_check)
- [triple_dist.sql](#triple_dist)

## angle_p_analysis

SQL: [angle_p_analysis.sql](sql/angle_p_analysis.sql)

Reports the 50th, 90th, 95th, and 99th percentiles of absolute timing error (converted from seconds to milliseconds) and absolute timing-angle error for batted results. Uses the ordered value at `floor((n - 1) * percentile) + 1`, without interpolation.

Output example: [angle_p_analysis_20260920_1.csv](data/angle_p_analysis_20260920_1.csv)

```csv
timing_p50_ms,timing_p90_ms,timing_p95_ms,timing_p99_ms,angle_p50_deg,angle_p90_deg,angle_p95_deg,angle_p99_deg
6.44229283540832,10.8176166424595,12.4965980652858,15.9253007930667,11.1934496820533,18.7641482315735,21.5837844705948,27.6206020068896
```

## avg_launch_speed

SQL: [avg_launch_speed.sql](sql/avg_launch_speed.sql)

Groups batted results on the tip side of the bat (`0 <= length_offset_m < 0.16`) into normalized offset bands. Reports contact counts, average launch speed overall and separately for fair/foul results, average distance, and average absolute final HLA. Speeds are in m/s; a fair/foul average is NULL when that subgroup has no rows.

Output example: [avg_launch_speed_20260922_1.csv](data/avg_launch_speed_20260922_1.csv)

```csv
tip_band,contacts,avg_launch_speed,avg_fair_launch_speed,avg_foul_launch_speed,avg_polar_distance,avg_abs_hla
0.00-0.25,1404,33.06,34.12,3.0,54.95,12.66
0.25-0.50,1742,32.95,34.11,5.63,50.78,19.42
0.50-0.70,1896,33.76,35.11,12.76,54.86,28.14
0.70-0.80,1104,32.32,33.58,20.05,57.29,30.61
```

## base_hla_final_hla

SQL: [base_hla_final_hla.sql](sql/base_hla_final_hla.sql)

Compares the foul-zone prediction (`ABS(HLA) > 45`) from the base and final horizontal launch angles with the recorded Foul result. Returns one row of match and mismatch counts over batted results. **Schema note:** requires `base_hla_deg`, which is absent from the current batting-table DDL.

Output example: [base_hla_final_hla.sql_20260920_1.csv](data/base_hla_final_hla.sql_20260920_1.csv)

```csv
n,base_matches_result,final_matches_result,base_mismatches_result,final_mismatches_result
11977,11804,11835,173,142
```

## batting_result_ball_spec_dist

SQL: [batting_result_ball_spec_dist.sql](sql/batting_result_ball_spec_dist.sql)

Groups all batting rows by result and reports counts plus average exit speed (km/h), launch angle (degrees), distance (meters), and flight time (seconds). Sorts by count descending. Includes non-contact results and their stored values, such as zero launch speed for Take.

Output example: [batting_result_ball_spec_dist_20260915.csv](data/batting_result_ball_spec_dist_20260915.csv)

```csv
result,n,avg_exit_kmh,avg_launch_angle,avg_distance_m,avg_total_time_sec
Take,11815,0.0,0.0,0.0,0.0
StrikeSwung,9247,0.0,0.0,0.0,0.0
Strikeout,6834,0.0,0.0,0.0,0.0
Out,2809,81.3,-19.9,33.1,2.39
```

## batting_result_dist

SQL: [batting_result_dist.sql](sql/batting_result_dist.sql)

Counts every batting result and its percentage of all rows, ordered by count descending. The denominator includes Take, StrikeSwung, and other non-contact results; this is an event distribution, not batting average or a plate-appearance rate.

Output example: [batting_result_dist_20260921_2.csv](data/batting_result_dist_20260921_2.csv)

```csv
result,n,pct
Take,22138,33.59
StrikeSwung,17625,26.74
Strikeout,12874,19.53
Out,8508,12.91
```

## change_from_previous_pp_1cm

SQL: [change_from_previous_pp_1cm.sql](sql/change_from_previous_pp_1cm.sql)

Bins batted results with nonnegative length offset into 1 cm intervals. Reports foul rates and their percentage-point change from the previous populated bin. Flags fewer than 30 contacts as `small_sample`, otherwise an absolute change of at least 15 points as `large_jump`; remaining rows are `ok`. The first change is NULL.

Output example: [change_from_previous_pp_1cm_20260921_1.csv](data/change_from_previous_pp_1cm_20260921_1.csv)

```csv
offset_from_m,offset_to_m,contacts,fouls,foul_pct,change_from_previous_pp,check_result
0.0,0.01,334,9,2.69,,ok
0.01,0.02,336,6,1.79,-0.91,ok
0.02,0.03,369,19,5.15,3.36,ok
0.03,0.04,365,14,3.84,-1.31,ok
```

## change_from_previous_pp_2cm

SQL: [change_from_previous_pp_2cm.sql](sql/change_from_previous_pp_2cm.sql)

Bins batted results with nonnegative length offset into 2 cm intervals. Reports foul percentage and adjusted SolidContact, WeakContact, and combined MarginalContact/FoulTip percentages, all divided by contacts in that bin. Despite the filename, this query does not calculate a change from the previous bin.

Output example: [change_from_previous_pp_2cm_20260921_1.csv](data/change_from_previous_pp_2cm_20260921_1.csv)

```csv
offset_from_m,offset_to_m,contacts,fouls,foul_pct,solid_pct,weak_pct,marginal_pct
0.0,0.02,670,15,2.24,42.99,42.24,14.78
0.02,0.04,734,33,4.5,43.46,39.37,17.17
0.04,0.06,827,34,4.11,42.32,41.6,16.08
0.06,0.08,915,37,4.04,45.79,40.0,14.21
```

## contact_type_around_tip

SQL: [contact_type_around_tip.sql](sql/contact_type_around_tip.sql)

Examines batted results at normalized tip offsets from 0.90 inclusive to 1.00 exclusive (0.144–0.16 m), restricted to adjusted SolidContact, WeakContact, or MarginalContact. Groups by adjusted type and reports fair/foul counts and percentages, average speed (m/s), absolute final HLA, and launch angle.

Output example: [contact_type_around_tip_20260922_1.csv](data/contact_type_around_tip_20260922_1.csv)

```csv
adjusted_contact_type,contacts,fouls,fair_balls,foul_pct,fair_pct,avg_launch_speed,avg_abs_hla,avg_launch_angle
SolidContact,318,8,310,2.52,97.48,37.94,31.87,-26.89
WeakContact,392,13,379,3.32,96.68,30.16,32.25,-19.66
MarginalContact,505,228,277,45.15,54.85,17.09,35.82,-13.94
```

## contact_type_around_tip_detail

SQL: [contact_type_around_tip_detail.sql](sql/contact_type_around_tip_detail.sql)

Splits batted results near the tip into normalized bands 0.80–0.90, 0.90–0.95, and 0.95–1.00. Reports fair/foul counts, foul rate, adjusted contact-type percentages, average speed (m/s), and absolute final HLA. Here `marginal_pct` counts only MarginalContact, not FoulTip.

Output example: [contact_type_around_tip_detail_20260922_2.csv](data/contact_type_around_tip_detail_20260922_2.csv)

```csv
tip_band,contacts,fouls,fair_balls,foul_pct,solid_pct,weak_pct,marginal_pct,avg_launch_speed,avg_abs_hla
0.80-0.90,1213,176,1037,14.51,34.21,32.73,33.06,29.76,32.11
0.90-0.95,611,107,504,17.51,23.9,33.22,42.88,27.62,32.74
0.95-1.00,641,153,488,23.87,21.37,26.21,52.42,26.02,33.34
```

## contact_type_dist

SQL: [contact_type_dist.sql](sql/contact_type_dist.sql)

Counts each original-to-adjusted contact-type pair across all batting rows. Orders alphabetically by original and adjusted types; useful for seeing which classifications changed after adjustment.

Output example: [contact_type_dist_20260921_1.csv](data/contact_type_dist_20260921_1.csv)

```csv
original_contact_type,adjusted_contact_type,n
MarginalContact,MarginalContact,1119
SolidContact,SolidContact,3527
SolidContact,WeakContact,396
SwungAndMiss,SwungAndMiss,36245
```

## contact_type_transition

SQL: [contact_type_transition.sql](sql/contact_type_transition.sql)

Groups all rows with nonnegative length offset by normalized tip band and original-to-adjusted contact-type pair. `pct_within_original` uses the total for that original type within that band as its denominator. Includes offsets beyond the tip and has no batted-result filter. The final split is at 1.23125 even though the labels abbreviate it to 1.23.

Output example: [contact_type_transition_20260921_1.csv](data/contact_type_transition_20260921_1.csv)

```csv
tip_band,original_contact_type,adjusted_contact_type,n,pct_within_original
0.00-0.50,MarginalContact,MarginalContact,484,100.0
0.00-0.50,SolidContact,SolidContact,1376,99.71
0.00-0.50,SolidContact,WeakContact,4,0.29
0.00-0.50,SwungAndMiss,SwungAndMiss,2765,100.0
```

## fair_with_high_offset

SQL: [fair_with_high_offset.sql](sql/fair_with_high_offset.sql)

Summarizes batted results at normalized tip offsets from 0.90 inclusive to 1.00 exclusive, excluding adjusted SwungAndMiss. Returns one row with fair/foul counts and rates, average speed (m/s), and average absolute final HLA.

Output example: [fair_with_high_offset_20260921_1.csv](data/fair_with_high_offset_20260921_1.csv)

```csv
contacts,fouls,fair_balls,foul_pct,fair_pct,avg_launch_speed,avg_abs_hla
1215,249,966,20.49,79.51,26.76,33.64
```

## foul_all_dist

SQL: [foul_all_dist.sql](sql/foul_all_dist.sql)

Returns total batted-result contacts, Foul count, and foul percentage of those contacts. Unlike the HLA-based summaries, excludes non-contact results from the denominator.

Output example: [foul_all_dist_20260921_3.csv](data/foul_all_dist_20260921_3.csv) (export note: `const MAX_LENGTH_OFFSET_M: f64 = 0.35`)

```csv
all_contacts,fouls,foul_pct_of_contacts
13516,1505,11.13
```

## foul_classification_dist

SQL: [foul_classification_dist.sql](sql/foul_classification_dist.sql)

For every row with non-NULL final HLA, cross-tabulates whether the angle lies outside ±45 degrees and whether the recorded result is Foul. Reports counts and percentages of all selected rows, ordered by count descending. The label `inside_and_fair` also includes non-contact results with an inside angle; it does not imply an actual fair batted ball.

Output example: [foul_classification_dist_20260920_1.csv](data/foul_classification_dist_20260920_1.csv)

```csv
classification,n,pct
inside_and_fair,64156,99.62
outside_and_foul,101,0.16
inside_but_foul,72,0.11
outside_but_not_foul,70,0.11
```

## fouls_hla_foul_zone_dist

SQL: [fouls_hla_foul_zone_dist.sql](sql/fouls_hla_foul_zone_dist.sql)

For all rows with non-NULL final HLA, compares recorded foul count/rate with the count/rate outside ±45 degrees. These rows may include non-contact results. **Schema note:** the CTE selects `base_hla_deg`, absent from the current DDL, even though the final aggregation does not use it.

Output example: [fouls_hla_foul_zone_dist_20260920_1.csv](data/fouls_hla_foul_zone_dist_20260920_1.csv)

```csv
contacts,fouls,foul_pct_of_contacts,hla_foul_zone,hla_foul_zone_pct
64399,173,0.27,171,0.27
```

## hla_dist

SQL: [hla_dist.sql](sql/hla_dist.sql)

Groups rows with non-NULL final HLA by absolute angle band and result. Reports counts and each result’s percentage within its angle band. Bands are 0–15, 15–30, 30–40, 40–45, 45–60, 60–90, and 90+ degrees; includes non-contact results with stored angles.

Output example: [hla_dist_20260920_1.csv](data/hla_dist_20260920_1.csv)

```csv
hla_band,result,n,pct_within_band
00-15,Take,21856,40.22
00-15,StrikeSwung,17517,32.24
00-15,Strikeout,12230,22.51
00-15,Out,1260,2.32
```

## homerun_dist

SQL: [homerun_dist.sql](sql/homerun_dist.sql)

Groups HomeRun results by exit speed rounded to the nearest 10 km/h and launch angle truncated to 5-degree multiples. Reports count and average distance (meters), sorted by speed and angle. Speed bins use rounding rather than a lower-bound interval.

Output example: [homerun_dist_20260915.csv](data/homerun_dist_20260915.csv)

```csv
exit_kmh_bin,angle_bin,n,avg_distance_m
120.0,35,2,111.4
120.0,40,1,100.2
120.0,45,3,107.6
130.0,25,3,115.7
```

## invalid_contacts_beyond_tip

SQL: [invalid_contacts_beyond_tip.sql](sql/invalid_contacts_beyond_tip.sql)

Counts rows whose length offset is strictly greater than 0.16 m but whose adjusted type is not SwungAndMiss. A zero count passes this diagnostic. Exactly 0.16 m is excluded, and there is no result filter.

Output example: [invalid_contacts_beyond_tip_20260921_1.csv](data/invalid_contacts_beyond_tip_20260921_1.csv)

```csv
invalid_contacts_beyond_tip
0
```

## monotony

SQL: [monotony.sql](sql/monotony.sql)

Checks how adjusted contact quality changes across increasing normalized tip-offset bands. Uses rows with nonnegative offsets and original type SolidContact, WeakContact, MarginalContact, FoulTip, or SwungAndMiss. Contact-quality percentages exclude adjusted misses; attempts include them. Flags a solid share increase above 3 percentage points, otherwise a weak/marginal share decrease below −3 points, relative to the previous populated band. Weak/marginal includes FoulTip. Bands 1–8 are 0–0.25, 0.25–0.50, 0.50–0.70, 0.70–0.80, 0.80–0.90, 0.90–1.00, 1.00–1.23125, and 1.23125+. NULL comparisons fall through to `ok`, so that label alone does not establish sufficient data.

Output example: [monotony_20260921_1.csv](data/monotony_20260921_1.csv)

```csv
band_no,attempts,contacts,solid_pct,solid_change_pp,weak_or_marginal_pct,weak_or_marginal_change_pp,check_result
1,2856,1404,43.23,,56.77,,ok
2,3055,1742,44.14,0.91,55.86,-0.91,ok
3,2862,1896,50.47,6.33,49.53,-6.33,solid_increase
4,1580,1104,43.39,-7.09,56.61,7.09,ok
```

## normalized_tip_offset_contact

SQL: [normalized_tip_offset_contact.sql](sql/normalized_tip_offset_contact.sql)

Intended to summarize attempts, contacts, adjusted contact-type counts, contact-quality percentages, and miss rate across normalized tip-offset bands. Uses nonnegative offsets and the same five original types as `monotony.sql`; combines MarginalContact and FoulTip. Quality percentages divide by contacts, while miss percentage divides by attempts; quality percentages are NULL with zero contacts. **Syntax note:** the file currently begins with `normalized_tip_offsetWITH` instead of `WITH` and cannot run as written. The saved output below illustrates the intended result. The last band boundary is 1.23125, abbreviated to 1.23 in labels.

Output example: [normalized_tip_offset_contact_20260921_1.csv](data/normalized_tip_offset_contact_20260921_1.csv)

```csv
tip_band,attempts,contacts,solid_n,weak_n,marginal_n,miss_n,solid_pct_of_contacts,weak_pct_of_contacts,marginal_pct_of_contacts,miss_pct
0.00-0.25,2856,1404,607,572,225,1452,43.23,40.74,16.03,50.84
0.25-0.50,3055,1742,769,710,263,1313,44.14,40.76,15.1,42.98
0.50-0.70,2862,1896,957,683,256,966,50.47,36.02,13.5,33.75
0.70-0.80,1580,1104,479,400,225,476,43.39,36.23,20.38,30.13
```

## outbound_result_dist

SQL: [outbound_result_dist.sql](sql/outbound_result_dist.sql)

Counts each outbound-ball classification and recorded batting-result pair across all rows. Sorts by outbound result and then count descending. Includes non-contact events, which can retain an InField classification.

Output example: [outbound_result_dist_20260915.csv](data/outbound_result_dist_20260915.csv)

```csv
outbound_result,result,n
Foul,Foul,7
GroundRuleDouble,Double,98
HomeRun,HomeRun,418
InField,Take,11815
```

## seconds_milliseconds_check

SQL: [seconds_milliseconds_check.sql](sql/seconds_milliseconds_check.sql)

Checks timing-error units on batted results with non-NULL timing error, bat speed, and timing angle. Computes expected absolute angular error using `bat_speed / 1.10 * timing_error * 180 / pi`, once treating timing error as seconds and once dividing by 1000 to treat it as milliseconds. Reports actual/expected averages and mean absolute errors; the smaller error indicates the closer unit interpretation. The saved example matches seconds.

Output example: [seconds_milliseconds_check_20260920_1.csv](data/seconds_milliseconds_check_20260920_1.csv)

```csv
n,avg_actual_angle_deg,avg_expected_if_seconds,avg_expected_if_ms,error_if_seconds,error_if_ms
11977,11.2858,11.2858,0.011286,0.0,11.27448
```

## short_distance_triples

SQL: [short_distance_triples.sql](sql/short_distance_triples.sql)

Lists Triple results with `polar_distance < 50` meters, joining batting to fielding on game/count and fielding to running on game/count/sequence. Shows distance, first-bounce distance, fielding time, defense time, and throw target. Multiple matched sequences can produce multiple rows for one triple; no ordering is specified. **Schema note:** the query references `time_to_fields`, but the current fielding-table DDL defines `time_to_field`.

Illustrative output only (invented values; no saved CSV). This is the expected shape on a schema with `time_to_fields`; the current schema produces a missing-column error. Column names are unaliased SQL expressions.

```csv
"ROUND(polar_distance, 2)","ROUND(first_bounce_distance, 2)","ROUND(time_to_fields, 2)","ROUND(defense_time, 2)",throw_target_base
42.5,31.2,3.4,5.8,Third
```

## signed_hla_dist

SQL: [signed_hla_dist.sql](sql/signed_hla_dist.sql)

Counts rows with non-NULL final HLA in signed angle bands, preserving negative/positive direction to inspect asymmetry. Reports percentages of all selected rows and sorts from the most negative band to the most positive. Includes non-contact results with stored angles.

Output example: [signed_hla_dist_20260920_1.csv](data/signed_hla_dist_20260920_1.csv)

```csv
hla_band,n,pct
-60--45,81,0.13
-45--30,4157,6.46
-30--15,1724,2.68
-15-0,1042,1.62
```

## timing_band_analysis

SQL: [timing_band_analysis.sql](sql/timing_band_analysis.sql)

Groups batted results by absolute timing error after converting seconds to milliseconds. Reports average absolute timing/base/final angles, counts crossing from inside ±45 degrees to outside or vice versa, and recorded fouls. Timing bands are 0–5, 5–10, 10–15, 15–20, 20–25, 25–30, 30–40, and 40+ ms. **Schema note:** requires `base_hla_deg`, absent from the current DDL.

Output example: [timing_band_analysis_20260920_1.csv](data/timing_band_analysis_20260920_1.csv)

```csv
timing_band,n,avg_timing_angle_deg,avg_abs_base_hla_deg,avg_abs_final_hla_deg,crossed_into_foul,crossed_into_fair,foul_results
00-05ms,4111,4.91,4.91,17.28,0,0,7
05-10ms,6117,12.75,12.75,32.05,0,0,9
10-15ms,1539,20.09,20.09,39.36,8,0,56
15-20ms,196,28.46,28.46,46.43,149,0,90
```

## timing_error_dist_dist

SQL: [timing_error_dist_dist.sql](sql/timing_error_dist_dist.sql)

Groups every row with non-NULL timing error by absolute raw timing-error band and result. Reports counts, mean absolute final HLA, and the percentage outside ±45 degrees. **Unit note:** labels say milliseconds, but the query compares raw `timing_error` directly with 5, 10, etc.; it does not multiply seconds by 1000. With the seconds-valued data shown here, normal errors collect in `00-05ms`. Includes non-contact results.

Output example: [timing_error_dist_dist_20260920_1.csv](data/timing_error_dist_dist_20260920_1.csv)

```csv
timing_band,result,n,avg_abs_final_hla_deg,hla_foul_zone_pct
00-05ms,Take,21856,0.0,0.0
00-05ms,StrikeSwung,17517,0.0,0.0
00-05ms,Strikeout,12230,0.0,0.0
00-05ms,Out,8990,28.81,0.71
```

## timing_error_unit_check

SQL: [timing_error_unit_check.sql](sql/timing_error_unit_check.sql)

Reports count, minimum, maximum, signed mean, and mean absolute raw timing error for batted results. Helps inspect scale before interpreting timing histograms. The saved values are on a seconds scale (mean absolute 0.006505 seconds ≈ 6.505 ms).

Output example: [timing_error_unit_check_20260920_1.csv](data/timing_error_unit_check_20260920_1.csv)

```csv
n,min_raw,max_raw,avg_raw,avg_abs_raw
11977,-0.0213990676942285,0.0221213299460076,-0.003032,0.006505
```

## triple_dist

SQL: [triple_dist.sql](sql/triple_dist.sql)

Groups Triple results by fielder position, reporting triple count, average distance (meters), average ball time (seconds), and average absolute polar angle (degrees). Sorts by triple count descending.

Output example: [triple_dist_20260915_2.csv](data/triple_dist_20260915_2.csv)

```csv
fielder_position,triples,avg_distance_m,avg_ball_time,avg_abs_angle
RF,1303,11.9,1.76,26.4
CF,276,26.6,2.98,30.3
LF,218,10.3,1.76,27.4
```
