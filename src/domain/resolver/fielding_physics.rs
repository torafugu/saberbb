use crate::domain::resolver::fielding_config::{FENCE_BOUNCE_COEFF, FIRST_BOUNCE_TIME};
use crate::domain::resolver::fielding_resolver::BoundedBallResult;
use crate::domain::shared::ball::TrajectoryType;
use crate::domain::shared::ball::{BattedBall, FieldedBall};
use crate::domain::shared::game_state::ActiveFielder;
use crate::domain::shared::stadium::Stadium;
use crate::domain::util::calculate_polar_distance;

pub fn try_catch(fielder: &ActiveFielder, ball: &BattedBall, stadium: &Stadium) -> FieldedBall {
    // $$\text{arrival\_time} = \text{reaction\_time} + \frac{\text{required\_distance}}{\text{fielder\_speed}}$$
    // 1. Calculate straight-line distance from position to landing point
    let required_distance = calculate_polar_distance(&fielder.polar_position, &ball.polar_position);
    let dy = fielder.polar_position.y - ball.y();

    // 3. Adjust initial reaction speed based on hit type (secret ingredient)
    let mut final_reaction = fielder.info.reaction;
    if ball.trajectory == TrajectoryType::Liner && dy < 0.0 {
        // Delay reaction when moving forward on a liner (harder to judge)
        final_reaction += fielder.info.reaction;
    }

    // 4. Calculate arrival time (seconds)
    let arrival_time = final_reaction + (required_distance / fielder.info.running_speed);

    // 5. Compare arrival time vs hang time
    if ball.trajectory == TrajectoryType::Grounder {
        return FieldedBall {
            ball: ball.clone(),
            fielded_by: fielder.position,
            time_to_field: arrival_time,
            is_fly_catch: false,
        };
    }

    if arrival_time <= ball.hang_time {
        return FieldedBall {
            ball: ball.clone(),
            fielded_by: fielder.position,
            time_to_field: ball.hang_time, // Fielder need to wait until catch.
            is_fly_catch: true,
        };
    }

    let bounded_ball = process_bounded_ball(fielder, ball, stadium);

    let mut final_ball = ball.clone();
    final_ball.polar_position.distance = bounded_ball.final_distance;

    FieldedBall {
        ball: final_ball,
        fielded_by: fielder.position,
        time_to_field: bounded_ball.time_to_fumble,
        is_fly_catch: false,
    }
}

// Processing when a fly/liner wasn't caught (became a hit)
fn process_bounded_ball(
    fielder: &ActiveFielder,
    ball: &BattedBall,
    stadium: &Stadium,
) -> BoundedBallResult {
    // 1. Damping coefficient at the moment of the first bounce (liner bounces sharply, fly dies)
    let k_impact = match ball.trajectory {
        TrajectoryType::Liner => 0.60,
        TrajectoryType::Fly => 0.35,
        _ => 0.0,
    };

    // 2. Initial speed as a grounder right after the bounce
    let v_horizontal = ball.launch_speed_ms() * ball.azimuth().cos() * 0.7; // Velocity including in-flight air resistance
    let v_bounce = v_horizontal * k_impact;

    // 3. Additional rolling distance and time until stop
    let roll_distance = v_bounce * 1.8;

    // 4. Provisional final resting position (landing point + roll distance)
    let mut final_distance = ball.distance() + roll_distance;

    // The fence bounce (cushion) logic
    if let Some(fence_distance) = stadium.fence_distance_at_angle(ball.angle()) {
        if final_distance > fence_distance {
            let overflow = final_distance - fence_distance;
            final_distance = fence_distance - (overflow * FENCE_BOUNCE_COEFF);
        }
    }

    // 5. Defense: time for the fielder to chase down and pick up the rolling ball
    // The fielder was initially running toward the landing point but didn't make it.
    // Simple calculation of time to loop around toward the direction the ball rolled (final_distance)
    let fielder_distance_to_ball = (final_distance - fielder.distance()).abs();

    // Time for the fielder to reach the final resting point (or cushion treatment position)
    let fielder_arrival_time =
        fielder.info.reaction + (fielder_distance_to_ball / fielder.info.running_speed);

    // Time the fielder picks up the ball (either waiting for it to stop or cutting it off mid-roll)
    let time_to_pick_up = fielder_arrival_time.max(ball.hang_time + FIRST_BOUNCE_TIME);
    BoundedBallResult {
        final_distance,
        time_to_fumble: time_to_pick_up, // NOTE: This becomes the time_to_field for the next throw play!
    }
}
