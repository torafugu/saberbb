# Running Logic

Source: `src/domain/resolver/running_resolver.rs`

This resolver decides runner movement by comparing the defense's elapsed play time with the runner's calculated travel time. `judge()` returns `Safe` when `defense_time - runner_time >= 0.0`; otherwise it returns `Out`. Most hit and tag-up paths return `RunnersUnsaved` so callers can commit the new base state later with `commit_unsaved_runners()`.

## Constants

- `ACCELERATION_LAG_TO_FIRST_BASE`: extra batter-runner acceleration time to first base.
- `ACCELERATION_LAG_AFTER_FIRST_BASE`: extra acceleration time for each base after first.
- `ACCELERATION_LAG_FROM_FIRST_TO_SECOND_BASE`: first-to-second lag constant, currently declared but not used directly.
- `ACCELERATION_LAG_FROM_FIRST_TO_THIRD_BASE`: first-to-third lag constant, currently declared but not used directly.

## Data Types

- `RunningEvent`: displayable event labels for tag-up, steal, double play, and grounder play.
- `StealRunnerAdvanceResult`: result payload for a steal attempt.
- `DoublePlayRunnerAdvanceResult`: result payload for the second throw of a double play.
- `RunnerAdvanceResult`: common result payload for batted-ball runner advancement.
- `RunningPlan`: target bases for the batter-runner and existing runners after an outfield hit.
- `RunnersUnsaved`: staged base occupancy before it is committed to `RunnersOnBase`.
- `HitAttemptResult`: internal result for resolving an outfield hit throw attempt.
- `RunnersOnBase`: current batter-runner and base-runner state.

## Function List And Logic Summary

### `RunningEvent::fmt()`

Converts a `RunningEvent` into its localized display label through `t!()`.

### `RunningPlan::set(defence_time, batter_to_second_time, batter_to_third_time)`

Chooses the expected outfield-hit plan from defensive delay:

- If defense time is longer than the batter's third-base time, plan a triple.
- Else if defense time is longer than the batter's second-base time, plan a double.
- Otherwise plan a single.

Existing runners are advanced optimistically according to that hit size. Inside-the-park home runs and running strategy variations are not modeled here.

### `RunnersUnsaved::put(base, runner)`

Stages a runner on first, second, or third. Home is ignored because scoring is tracked separately.

### `RunnersUnsaved::put_if_some(base, runner)`

Stages an optional runner only when it is `Some`. Home is ignored.

### `RunnersUnsaved::score_if_some(runner)`

Returns `1` if a runner exists and `0` otherwise. Used to count scored runners.

### `RunnersUnsaved::runner_id_on(base)`

Returns the runner id staged on first, second, or third. Returns `None` for empty bases and home.

### `advance_count(from, to)`

Maps a legal base path to the number of bases advanced. Rejects unsupported paths, reverse movement, same-base movement, and batter-runner home-to-home movement with `GameError::UnsupportedPath`.

### `judge(defense_time, runner_time)`

Computes `time_difference = defense_time - runner_time`. The runner is safe when the difference is zero or positive, and out when the defense arrives first.

### `HitAttemptResult::new()`

Creates a default internal hit-attempt result. This constructor is currently unused by the resolver.

### `RunnersOnBase::empty()`

Clears the batter-runner and all occupied bases.

### `RunnersOnBase::has_runner_on(base)`

Checks whether first, second, or third is occupied. Home always returns `false`.

### `RunnersOnBase::is_loaded()`

Returns true when first, second, and third are all occupied.

### `RunnersOnBase::has_first_and_second()`

Returns true when first and second are occupied.

### `RunnersOnBase::has_second_and_third()`

Returns true when second and third are occupied.

### `RunnersOnBase::has_first_and_third()`

Returns true when first and third are occupied.

### `RunnersOnBase::current_runners()`

Copies the current first, second, and third runners into a `RunnersUnsaved` staging object.

### `RunnersOnBase::runner_on(base)`

Fetches the runner at a specific origin base:

- Home means the batter-runner.
- First, second, and third mean existing base runners.
- Missing runners return the matching `GameError`.

### `RunnersOnBase::batter_runner_time_to(to_base, with_lag, batting_side)`

Calculates batter-runner travel time from home to the target base:

- Uses `advance_count(Home, to_base)` for distance.
- Adds a 2.0 distance penalty for right-handed batters.
- Adds acceleration lag when `with_lag` is true.
- Divides total distance by batter-runner speed.

### `RunnersOnBase::runner_advance_time(runner, base_count, with_lag)`

Calculates travel time for an existing runner:

- Distance is `BASE_DISTANCE * base_count`.
- Lead distance is subtracted from the route.
- Optional acceleration lag is added per base.
- Final time is adjusted distance divided by runner speed.

### `RunnersOnBase::steal_base_runner_time(from_base, to_base)`

Calculates steal time by adding the runner's `start_reaction` to `total_runner_time()`.

### `RunnersOnBase::total_runner_time(from_base, to_base)`

Calculates normal advancement time for an existing runner. It rejects same-base targets and batter-runner movement from home, fetches the origin runner, converts the path to base count, then calls `runner_advance_time()` with lag enabled.

### `RunnersOnBase::after_homerun()`

Scores the batter plus all occupied bases, clears every runner immediately, and returns the run count. It does not need `commit_unsaved_runners()` because the state is mutated directly.

### `RunnersOnBase::commit_unsaved_runners(unsaved_runners)`

Replaces the current first, second, and third base state with staged runners. The batter-runner field is not changed here.

### `RunnersOnBase::after_infield_grounder(defense_play_result, batting_side)`

Resolves runner movement after an infield grounder based on the throw target:

- Throw home: force batter to first, first runner to second, second runner to third, and judge the third-base runner at home if present.
- Throw third: force batter to first and first runner to second, judge the second-base runner at third if present, and score the third-base runner.
- Throw second: force batter to first, judge the first-base runner at second if present, move second runner to third, and score the third-base runner.
- Throw first: judge the batter-runner at first, record a single if safe, force first runner to second and second runner to third, and score the third-base runner.

The result includes the target runner, ruling, batting result, runs scored, and staged runners.

### `RunnersOnBase::resolve_triple_attempt(throw_target_base, defense_time, batting_side)`

Internal helper for an outfield-hit plan where the batter attempts third:

- Throw home targets the first-base runner trying to score when one exists.
- Throw third targets the batter-runner trying for third.
- Other throw targets produce no active runner race.

If the batter-runner is out at third, the batting result is downgraded to double. Otherwise the hit remains a triple.

### `RunnersOnBase::resolve_double_attempt(throw_target_base, defense_time, batting_side)`

Internal helper for an outfield-hit plan where the batter attempts second:

- Throw home first judges the runner from second if present, then the runner from first if the earlier home attempt was safe.
- Throw third targets the runner from first.
- Throw second targets the batter-runner.

If the batter-runner is out at second, the batting result is downgraded to single. Other outcomes keep the hit as a double.

### `RunnersOnBase::resolve_single_attempt(throw_target_base, defense_time, batting_side)`

Internal helper for an outfield-hit plan where the batter attempts first:

- Throw home targets the third-base runner.
- Throw third targets the second-base runner.
- Throw second targets the first-base runner.
- Throw first targets the batter-runner.

If the batter-runner is out at first, the batting result is out. Otherwise it remains a single.

### `RunnersOnBase::score_for_existing_runners(batter_target_base, from_base, ruling)`

Counts runs for existing runners after an outfield hit attempt:

- Triple plan: third and second always score; first scores only if the contested play is safe.
- Double plan: third always scores; second and first score when the contested play is safe, with special handling when the first-base runner is the one thrown out.
- Single plan: third scores unless the throw home retires that third-base runner.

Inside-the-park home run paths are rejected.

### `RunnersOnBase::build_runner_advance_result(batter_target_base, from_base, ruling)`

Builds staged base occupancy after an outfield hit:

- Triple plan: batter is staged on third only when safe.
- Double plan: batter is staged on second when safe; if a runner from second is out at home, the batter still reaches second and the first-base runner reaches third.
- Single plan: existing runners advance unless they were the runner thrown out, and the batter reaches first unless out at first.

Scored runners are omitted from the staged bases.

### `RunnersOnBase::after_outfield_hit(defense_play_result, batting_side)`

Resolves an outfield hit end to end:

1. Calculates no-lag batter times to second and third.
2. Chooses a `RunningPlan` from defense time.
3. Uses the matching single, double, or triple attempt resolver.
4. Counts scored runners.
5. Builds staged base occupancy.
6. Returns a `RunnerAdvanceResult`.

Inside-the-park home runs are not supported.

### `RunnersOnBase::after_tagup(defense_play_result)`

Resolves tag-up advancement:

- Throw home judges the third-base runner scoring; first and second hold.
- Throw third judges the second-base runner advancing; first holds and the third-base runner scores.
- Other throws keep first and second in place and score the third-base runner.

The batting result remains out because the ball in play was caught.

### `RunnersOnBase::after_double_play(double_play_defense_play_result, previous_runner_advanced_result, batting_side)`

Resolves the second throw of a double play using the staged runners from the first play:

- Throw first targets the batter-runner.
- Throw second targets the first-base runner.
- Throw third targets the second-base runner.

If the defense wins the second race, the target staged runner is removed and batting result becomes `DoublePlay`. Throwing home is rejected.

### `RunnersOnBase::after_base_stealing(steal_defense_play_result)`

Resolves a steal attempt and mutates base state immediately:

- Throw third targets the runner from second stealing third.
- Throw second targets the runner from first stealing second.
- Other targets are rejected.

On a safe ruling, the runner is moved to the target base and removed from the origin base. Home steals and double steals are not supported.

## Test Helper Functions

The test module also defines helper constructors: `assert_near()`, `runner()`, `runner_with_lead()`, `runners()`, `defense_result()`, `double_play_result()`, `steal_result()`, and `runner_advance_result()`. These exist only to build compact unit test fixtures and are not part of the runtime resolver logic.
