# Fielding Logic

# Fielding Process Flow
<img src="../images/process_fielding.png" width="30%">

### 1. is_ball_in_fielder_lane

Determine which fielders can catch the ball along the batted-ball trajectory.

#### Catchable Range by Fielder

| Fielder type | Catchable angle |
| ---- | ---- |
| Pitcher | 4.0° |
| First baseman, third baseman | 6.0° |
| Second baseman, shortstop | 8.0° |
| Outfielder | 12.0° |

$$\theta_{\text{coverage}} = \theta_{\text{coverage}} \cdot (1.0 + \text{FielderInfo.reach\_range} \cdot 0.05)$$

#### Determining Whether the Ball Is Catchable

$$\theta_{\text{fielder}} - \theta_{\text{ball}} \le \theta_{\text{coverage}}$$

### 2. evaluate_fielder_interception

Stop the ball mid-play while it is in flight or bouncing as a ground ball (fly-ball catch, ground-ball fielding).

#### Get the Current Position of the Batted Ball
#### Move to the Future Position Where the Fielder Can Catch the Ball
1. Determine travel time
- Calculate the time $t_{\text{fielder}}$ required for the fielder to move from the initial position to the ball's horizontal position $(X(t), Y(t))$.
$$t_{\text{fielder}} = t_{\text{reaction}} + \frac{\text{Distance}((X_{\text{f0}}, Y_{\text{f0}}), (X(t), Y(t)))}{v_{\text{fielder}}}$$

2. If there is enough time to reach the ball's horizontal position, get there ahead of the ball
$$t_{\text{fielder}} \le t_{\text{ball}}$$
- If the fielder has enough time to get there first ($t_{\text{fielder}} < t$), the fielder waits at that point for the ball to come down and can catch it reliably before it bounces.

3. Determine whether the ball is at a catchable height
$$Z(t) \le Z_{\text{catch\_max}}$$

### 3. evaluate_final_pickup

If no fielder can stop the ball along its trajectory, the nearest fielder picks it up after it stops or slows near the fence or deep in the outfield.

#### Determining Pickup Time

$$t_{\text{pickup}} = \max(t_{\text{ball\_stop}}, t_{\text{fielder\_travel}}) + t_{\text{pickup\_delay}}$$

- $t_{\text{ball\_stop}}$: time when the ball stops
- $t_{\text{fielder\_travel}}$: time required for the fielder to run from the initial position to the final position $(X_{\text{final}}, Y_{\text{final}})$
- $t_{\text{pickup\_delay}}$: delay for handling a carom off the wall or picking up a stopped ball (about $0.3 \sim 0.5$ seconds)

#### If the Fielder Arrives Before the Ball Stops

$$t_{\text{pickup}} = t_{\text{fielder\_travel}} + t_{\text{ball\_stop}} + t_{\text{pickup\_delay}}$$
The fielder waits for the ball to stop before picking it up.

#### If the Fielder Arrives After the Ball Stops

$$t_{\text{pickup}} = t_{\text{fielder\_travel}} + t_{\text{pickup\_delay}}$$

### Error Judgment

| Error type | Trigger condition | Wait time | Ball position | Error handling |
| ---- | ---- | ---- | ---- | ---- |
| Fumble | Ball pops out of glove <br> Bobble | Stops in place or at the fielder's feet | Enough time | Add a penalty to processing time |
| Missed ball | Catch while charging <br> Backhand attempt <br> Forced dive | Just barely | Continues rolling backward | Move to backup handling |

#### Calculating Error Probability

$$\text{ErrorRate} = (1.0 - \text{FielderInfo.catching}) \cdot \text{DifficultyFactor}$$

- Elements of $\text{DifficultyFactor}$
  - Lack of waiting time (a just-barely catch is more difficult)
  - Batted-ball velocity $v$
  - Batted-ball height $Z$

### Risk Strategy for Catching Before a Bounce

- $Aggressive$ (challenge-first):
  - If there is a chance to catch the ball before it bounces, the fielder dives in aggressively even when wait time is almost zero, such as on a last-second diving attempt.
- $Balanced$:
  - Even if there is a chance to catch the ball before it bounces, if wait time is below a certain threshold, the fielder chooses a safer fielding point.
- $Conservative$ (safety-first):
  - The fielder does not force a charge. Instead, the fielder drops back to a safer point where the ball has settled after one bounce and there is enough wait time.
