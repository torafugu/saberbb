mod common;

use common::*;
use glam::DVec3;
use saberbb::domain::random_provider::*;
use saberbb::domain::resolver::pitching_resolver::*;
use saberbb::domain::schedule_service::ScheduleService;

#[test]
fn test_bat_angle() {
    let mut rng = RealRng::new();

    let pitcher = generate_pitcher();
    let base_four_seam_speed = ScheduleService::<
        saberbb::repositories::schedule_repository::SqlScheduleRepository,
    >::DEFAULT_BASE_FOUR_SEAM_SPEED;

    let hanging_pitch_effect = calculate_hanging_pitch_effect(&mut rng, &pitcher);
    let pitched_ball = create_pitch(
        &mut rng,
        &pitcher,
        hanging_pitch_effect,
        base_four_seam_speed,
    )
    .unwrap();

    let ball_velocity = pitched_ball
        .velocity_at_plate(&StrikeZoneDimensions::default())
        .unwrap();

    let bat_azimuth_deg: f64 = 20.0;
    let bat_tilt_deg: f64 = 10.0;
    let azimuth = bat_azimuth_deg.to_radians();
    let tilt = bat_tilt_deg.to_radians();

    let bat_axis = DVec3::new(
        tilt.cos() * azimuth.cos(),
        tilt.cos() * azimuth.sin(),
        tilt.sin(),
    );

    let bat_mass_properties = calculate_bat_mass_properties(&DEFAULT_WOOD_BAT_SLICES).unwrap();

    println!("Bat mass properties: {:?}", bat_mass_properties);

    let bat_angle = (40.0_f64).to_radians();
    let offset_x_m = 0.1;
    let offset_z_m = 0.05;

    let signed_thickness_offset_m = -offset_x_m * bat_angle.sin() + offset_z_m * bat_angle.cos();
    let signed_length_offset_m = offset_x_m * bat_angle.cos() + offset_z_m * bat_angle.sin();

    let pivot_point = DVec3::ZERO;
    let bat_origin = pivot_point - bat_axis * 0.15;

    let bat = Bat::default_wood();
    let bat_pose = BatPose {
        origin: bat_origin,
        axis: bat_axis,
    };

    let position_m = bat
        .contact_position_from_knob(signed_length_offset_m)
        .unwrap();

    let geometry = calculate_contact_geometry(
        &bat,
        bat_pose.axis,
        position_m, // nominal contact position from knob
        ball_velocity,
    )
    .unwrap();

    let effective_mass = effective_bat_mass(&bat, &geometry);

    let bat_velocity = calculate_bat_contact_velocity(bat_pose.axis, ball_velocity, 33.0).unwrap();

    let impact_result = resolve_cylindrical_impact(
        &bat,
        &geometry,
        ball_velocity,
        bat_velocity,
        0.145, // ball mass
        0.25,  // coefficient of restitution
    );

    println!("Impact result: {:?}", impact_result);
}
