use glam::DVec3;
use saberbb::domain::player_factory::PlayerFactory;
use saberbb::domain::player_service::PlayerService;
use saberbb::domain::resolver::fielding_physics::FielderRiskTolerance;
use saberbb::domain::shared::game_state::*;
use saberbb::domain::shared::player::*;
use saberbb::domain::shared::stadium::*;
use saberbb::domain::util::*;
use saberbb::repositories::player_repository::SqlPlayerRepository;

#[allow(dead_code)]
pub fn generate_stadium() -> Stadium {
    Stadium::new(1, "AAA".to_string(), 98.0, 120.0, 2.0)
}

#[allow(dead_code)]
pub fn generate_default_fielders() -> [ActiveFielder; 9] {
    let p = ActiveFielder {
        position: Position::P,
        id: 0,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(FielderType::Pitcher),
        polar_position: PolarPosition::new(MOUND_DISTANCE, 0.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let c = ActiveFielder {
        position: Position::C,
        id: 1,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(FielderType::Catcher),
        polar_position: PolarPosition::new(0.0, 0.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let fb = ActiveFielder {
        position: Position::FB,
        id: 2,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(
            FielderType::CornerInfielder,
        ),
        polar_position: PolarPosition::new(35.0, 33.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let sb = ActiveFielder {
        position: Position::SB,
        id: 3,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(
            FielderType::MiddleInfielder,
        ),
        polar_position: PolarPosition::new(40.0, 18.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let tb = ActiveFielder {
        position: Position::TB,
        id: 4,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(
            FielderType::CornerInfielder,
        ),
        polar_position: PolarPosition::new(35.0, -33.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let ss = ActiveFielder {
        position: Position::SS,
        id: 5,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(
            FielderType::MiddleInfielder,
        ),
        polar_position: PolarPosition::new(40.0, -18.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let rf = ActiveFielder {
        position: Position::RF,
        id: 6,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(FielderType::Outfielder),
        polar_position: PolarPosition::new(80.0, 26.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let cf = ActiveFielder {
        position: Position::CF,
        id: 7,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(FielderType::Outfielder),
        polar_position: PolarPosition::new(90.0, 0.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    let lf = ActiveFielder {
        position: Position::LF,
        id: 8,
        info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(FielderType::Outfielder),
        polar_position: PolarPosition::new(80.0, -26.0),
        risk_tolerance: FielderRiskTolerance::Balanced,
    };

    [p, c, fb, sb, tb, ss, rf, cf, lf]
}

pub fn generate_batter() -> BatterInfo {
    let player_service = PlayerService {
        repo: SqlPlayerRepository::new().expect("failed to initialize player repository"),
    };
    let mut player_factory = PlayerFactory::new(player_service);
    player_factory
        .load_player_probs()
        .expect("failed to load player probabilities");
    player_factory
        .assign_batter_info()
        .expect("failed to generate batter info")
}

pub fn generate_pitcher() -> PitcherInfo {
    let player_service = PlayerService {
        repo: SqlPlayerRepository::new().expect("failed to initialize player repository"),
    };
    let mut player_factory = PlayerFactory::new(player_service);
    player_factory
        .load_player_probs()
        .expect("failed to load player probabilities");

    loop {
        if let Some(pitcher_info) = player_factory
            .generate_player()
            .expect("failed to generate player")
            .defense_skills
            .pitcher
        {
            let mut pitcher_info = pitcher_info;
            pitcher_info.control = 10.0;
            return pitcher_info;
        }
    }
}

#[allow(dead_code)]
pub fn generate_catcher() -> CatcherInfo {
    CatcherInfo {
        fielder_info: PlayerFactory::<SqlPlayerRepository>::default_fielder_info(
            FielderType::Catcher,
        ),
    }
}

#[allow(dead_code)]
pub fn generate_runner() -> ActiveRunner {
    let player_service = PlayerService {
        repo: SqlPlayerRepository::new().expect("failed to initialize player repository"),
    };
    let mut player_factory = PlayerFactory::new(player_service);
    player_factory
        .load_player_probs()
        .expect("failed to load player probabilities");
    let player = player_factory
        .generate_player()
        .expect("failed to generate player");

    ActiveRunner {
        id: player.info.id,
        skills: player.offense_skills.running,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BatSlice {
    // Distance from the knob to the center of this slice.
    position_m: f64,

    // Length of this slice along the bat axis.
    width_m: f64,

    // Average radius of this slice.
    radius_m: f64,

    // Mass assigned to this slice.
    mass_kg: f64,
}

pub fn validate_slices(slices: &[BatSlice]) -> Option<()> {
    if slices.is_empty() {
        return None;
    }

    for slice in slices {
        if !slice.position_m.is_finite()
            || !slice.width_m.is_finite()
            || !slice.radius_m.is_finite()
            || !slice.mass_kg.is_finite()
            || slice.width_m <= 0.0
            || slice.radius_m <= 0.0
            || slice.mass_kg <= 0.0
        {
            return None;
        }
    }

    for pair in slices.windows(2) {
        if pair[0].position_m >= pair[1].position_m {
            return None;
        }
    }

    Some(())
}

// total_mass_kg              = 0.9000
// center_of_mass_m           = 0.5605
// transverse_inertia_kg_m2   = 0.04548
// inertia_about_knob_kg_m2   = 0.32819
pub const DEFAULT_WOOD_BAT_SLICES: [BatSlice; 8] = [
    BatSlice {
        position_m: 0.0525,
        width_m: 0.105,
        radius_m: 0.0170,
        mass_kg: 0.060,
    },
    BatSlice {
        position_m: 0.1575,
        width_m: 0.105,
        radius_m: 0.0145,
        mass_kg: 0.043,
    },
    BatSlice {
        position_m: 0.2625,
        width_m: 0.105,
        radius_m: 0.0150,
        mass_kg: 0.046,
    },
    BatSlice {
        position_m: 0.3675,
        width_m: 0.105,
        radius_m: 0.0170,
        mass_kg: 0.060,
    },
    BatSlice {
        position_m: 0.4725,
        width_m: 0.105,
        radius_m: 0.0210,
        mass_kg: 0.091,
    },
    BatSlice {
        position_m: 0.5775,
        width_m: 0.105,
        radius_m: 0.0270,
        mass_kg: 0.150,
    },
    BatSlice {
        position_m: 0.6825,
        width_m: 0.105,
        radius_m: 0.0330,
        mass_kg: 0.225,
    },
    BatSlice {
        position_m: 0.7875,
        width_m: 0.105,
        radius_m: 0.0330,
        mass_kg: 0.225,
    },
];

#[derive(Clone, Copy, Debug)]
pub struct BatMassProperties {
    pub mass_kg: f64,

    // Center of mass measured from the knob along the bat axis.
    pub center_of_mass_m: f64,

    // Transverse moment of inertia about the center of mass.
    pub transverse_inertia_kg_m2: f64,
}

#[derive(Clone, Debug)]
pub struct Bat {
    slices: Box<[BatSlice]>,
    mass_properties: BatMassProperties,

    length_m: f64,

    // Nominal sweet-spot position measured from the knob.
    reference_contact_from_knob_m: f64,

    // Approximate hand/pivot position measured from the knob.
    pivot_from_knob_m: f64,

    // Normal coefficient of restitution.
    restitution: f64,
}

impl Bat {
    pub fn new(
        slices: impl Into<Box<[BatSlice]>>,
        reference_contact_from_knob_m: f64,
        pivot_from_knob_m: f64,
        restitution: f64,
    ) -> Option<Self> {
        let slices = slices.into();

        validate_slices(&slices)?;

        let first = slices.first()?;
        let last = slices.last()?;

        let start_m = first.position_m - first.width_m / 2.0;

        let length_m = last.position_m + last.width_m / 2.0;

        // bat_origin represents the knob, so the first slice must
        // start at approximately zero.
        if start_m.abs() > 1e-6 {
            return None;
        }

        if !(0.0..=length_m).contains(&reference_contact_from_knob_m) {
            return None;
        }

        if !(0.0..=length_m).contains(&pivot_from_knob_m) {
            return None;
        }

        if !(0.0..=1.0).contains(&restitution) {
            return None;
        }

        let mass_properties = calculate_bat_mass_properties(&slices)?;

        Some(Self {
            slices,
            mass_properties,
            length_m,
            reference_contact_from_knob_m,
            pivot_from_knob_m,
            restitution,
        })
    }

    pub fn default_wood() -> Self {
        Self::new(DEFAULT_WOOD_BAT_SLICES, 0.70, 0.15, 0.50)
            .expect("default wood bat must be valid")
    }

    pub fn length_m(&self) -> f64 {
        self.length_m
    }

    pub fn mass_properties(&self) -> BatMassProperties {
        self.mass_properties
    }

    pub fn mass_kg(&self) -> f64 {
        self.mass_properties.mass_kg
    }

    pub fn center_of_mass_from_knob_m(&self) -> f64 {
        self.mass_properties.center_of_mass_m
    }

    pub fn transverse_inertia_kg_m2(&self) -> f64 {
        self.mass_properties.transverse_inertia_kg_m2
    }

    pub fn reference_contact_from_knob_m(&self) -> f64 {
        self.reference_contact_from_knob_m
    }

    pub fn pivot_from_knob_m(&self) -> f64 {
        self.pivot_from_knob_m
    }

    pub fn restitution(&self) -> f64 {
        self.restitution
    }

    pub fn contact_position_from_knob(&self, signed_length_offset_m: f64) -> Option<f64> {
        let position_m = self.reference_contact_from_knob_m + signed_length_offset_m;

        if !(0.0..=self.length_m).contains(&position_m) {
            return None;
        }

        Some(position_m)
    }

    pub fn radius_at(&self, position_m: f64) -> Option<f64> {
        if !(0.0..=self.length_m).contains(&position_m) {
            return None;
        }

        let first = self.slices.first()?;

        if position_m <= first.position_m {
            return Some(first.radius_m);
        }

        for pair in self.slices.windows(2) {
            let left = pair[0];
            let right = pair[1];

            if position_m <= right.position_m {
                let span = right.position_m - left.position_m;

                if span <= 0.0 {
                    return None;
                }

                let t = (position_m - left.position_m) / span;

                return Some(left.radius_m + (right.radius_m - left.radius_m) * t);
            }
        }

        self.slices.last().map(|slice| slice.radius_m)
    }

    pub fn radius_at_contact(&self, signed_length_offset_m: f64) -> Option<f64> {
        let position_m = self.contact_position_from_knob(signed_length_offset_m)?;

        self.radius_at(position_m)
    }
}

pub fn calculate_bat_mass_properties(slices: &[BatSlice]) -> Option<BatMassProperties> {
    let mass_kg: f64 = slices.iter().map(|slice| slice.mass_kg).sum();

    if mass_kg <= 0.0 {
        return None;
    }

    let center_of_mass_m = slices
        .iter()
        .map(|slice| slice.position_m * slice.mass_kg)
        .sum::<f64>()
        / mass_kg;

    let transverse_inertia_kg_m2 = slices
        .iter()
        .map(|slice| {
            let distance_from_com = slice.position_m - center_of_mass_m;

            slice.mass_kg * distance_from_com.powi(2)
        })
        .sum();

    if transverse_inertia_kg_m2 <= 0.0 {
        return None;
    }

    Some(BatMassProperties {
        mass_kg,
        center_of_mass_m,
        transverse_inertia_kg_m2,
    })
}

#[derive(Clone, Copy, Debug)]
pub struct BatPose {
    // World or collision-local position of the knob.
    pub origin: DVec3,

    // Unit vector from knob toward barrel.
    pub axis: DVec3,
}

impl BatPose {
    pub fn new(origin: DVec3, axis: DVec3) -> Option<Self> {
        Some(Self {
            origin,
            axis: axis.try_normalize()?,
        })
    }

    pub fn axis_point(&self, position_from_knob_m: f64) -> DVec3 {
        self.origin + self.axis * position_from_knob_m
    }

    pub fn center_of_mass(&self, bat: &Bat) -> DVec3 {
        self.axis_point(bat.center_of_mass_from_knob_m())
    }

    pub fn pivot_point(&self, bat: &Bat) -> DVec3 {
        self.axis_point(bat.pivot_from_knob_m())
    }
}

pub fn effective_bat_mass(bat: &Bat, geometry: &ContactGeometry) -> f64 {
    let axial_offset_from_com_m = geometry.position_from_knob_m - bat.center_of_mass_from_knob_m();

    let lever_arm_sq = axial_offset_from_com_m * axial_offset_from_com_m;

    let inverse_effective_mass = 1.0 / bat.mass_properties.mass_kg
        + lever_arm_sq / bat.mass_properties.transverse_inertia_kg_m2;

    1.0 / inverse_effective_mass
}

pub fn calculate_bat_contact_velocity(
    bat_axis: DVec3,
    incoming_ball_velocity: DVec3,
    bat_speed_at_contact_mps: f64,
) -> Option<DVec3> {
    let axis = bat_axis.try_normalize()?;

    let opposite_pitch_horizontal =
        DVec3::new(-incoming_ball_velocity.x, -incoming_ball_velocity.y, 0.0);

    let perpendicular_direction =
        opposite_pitch_horizontal - axis * opposite_pitch_horizontal.dot(axis);

    let direction = perpendicular_direction.try_normalize()?;

    Some(direction * bat_speed_at_contact_mps)
}

#[derive(Clone, Copy, Debug)]
pub struct ContactGeometry {
    /// Distance from the knob to the contact cross-section
    pub position_from_knob_m: f64,

    /// Point on the bat's center axis at the contact position
    pub axis_point: DVec3,

    /// Contact point on the bat surface
    pub bat_surface_point: DVec3,

    /// Contact point on the ball surface
    pub ball_surface_point: DVec3,

    /// Outward unit normal pointing from the bat's center axis toward the ball center
    pub normal: DVec3,
}

pub fn calculate_contact_geometry(
    bat: &Bat,
    bat_axis: DVec3,
    contact_position_from_knob_m: f64,
    relative_approach_velocity: DVec3,
) -> Option<ContactGeometry> {
    let bat_origin = DVec3::ZERO;

    let axis = bat_axis.try_normalize()?;

    let position_from_knob_m = contact_position_from_knob_m.clamp(0.0, bat.length_m);

    // Point on the bat's center axis at the contact position
    let axis_point = bat_origin + axis * position_from_knob_m;

    let bat_radius_m = bat.radius_at(position_from_knob_m)?;

    if !bat_radius_m.is_finite() || bat_radius_m <= 0.0 {
        return None;
    }

    // Remove the component along the bat axis from the incoming velocity
    let approach_perpendicular =
        relative_approach_velocity - axis * relative_approach_velocity.dot(axis);

    let approach_direction = approach_perpendicular.try_normalize()?;

    // Outward normal pointing from the bat axis toward the ball
    let normal = -approach_direction;

    let bat_surface_point = axis_point + normal * bat_radius_m;

    // For an ideal instantaneous collision the two surface points coincide
    let ball_surface_point = bat_surface_point;

    Some(ContactGeometry {
        position_from_knob_m,
        axis_point,
        bat_surface_point,
        ball_surface_point,
        normal,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct ImpactResult {
    /// Ball velocity after the collision
    pub outgoing_ball_velocity: DVec3,

    /// Magnitude of the impulse applied in the normal direction
    pub normal_impulse_ns: f64,

    /// Relative normal velocity before the collision
    pub relative_normal_velocity_before_mps: f64,

    /// Relative normal velocity after the collision
    pub relative_normal_velocity_after_mps: f64,

    /// Effective bat mass at this contact position
    pub effective_bat_mass_kg: f64,
}

/// Return None when there is no collision, or when the ball departs backward.
/// `bat_velocity` is the velocity of the bat *at the contact point*.
pub fn resolve_cylindrical_impact(
    bat: &Bat,
    geometry: &ContactGeometry,
    incoming_ball_velocity: DVec3,
    bat_contact_velocity: DVec3,
    ball_mass_kg: f64,
    coefficient_of_restitution: f64,
) -> Option<ImpactResult> {
    // const HALF_BAT_LENGTH_M: f64 = 0.4; // Half the length of the bat in meters
    // const BALL_RADIUS_M: f64 = 0.037; // Radius of the ball in meters
    // const BALL_MASS_KG: f64 = 0.145; // Mass of the ball in kilograms
    // const RESTITUTION: f64 = 0.25; // Typical coefficient of restitution for a baseball-bat collision

    if !ball_mass_kg.is_finite() || ball_mass_kg <= 0.0 {
        return None;
    }

    if !coefficient_of_restitution.is_finite() || coefficient_of_restitution < 0.0 {
        return None;
    }

    let normal = geometry.normal.try_normalize()?;

    let effective_bat_mass_kg = effective_bat_mass(bat, geometry);

    /*
     * Relative velocity of the ball as seen from the bat.
     *
     * The normal points from the bat toward the ball, so this is
     * negative while the ball is approaching the bat.
     */
    let relative_velocity_before = incoming_ball_velocity - bat_contact_velocity;

    let relative_normal_velocity_before_mps = relative_velocity_before.dot(normal);

    /*
     * If this is 0 or greater, the surfaces are already separating
     * along the normal direction, so no collision impulse may be applied.
     */
    if relative_normal_velocity_before_mps >= 0.0 {
        return None;
    }

    /*
     * One-dimensional collision with a coefficient of restitution.
     *
     * j =
     *   -(1 + e) * v_relative_normal
     *   --------------------------------
     *       1 / m_ball + 1 / m_bat_eff
     */
    let inverse_mass_sum = 1.0 / ball_mass_kg + 1.0 / effective_bat_mass_kg;

    let normal_impulse_ns = -(1.0 + coefficient_of_restitution)
        * relative_normal_velocity_before_mps
        / inverse_mass_sum;

    let outgoing_ball_velocity =
        incoming_ball_velocity + normal * (normal_impulse_ns / ball_mass_kg);

    /*
     * Diagnostics only.
     *
     * The relative normal velocity after the collision, assuming the
     * reaction were also applied to the bat.
     */
    let relative_normal_velocity_after_mps =
        relative_normal_velocity_before_mps + normal_impulse_ns * inverse_mass_sum;

    Some(ImpactResult {
        outgoing_ball_velocity,
        normal_impulse_ns,
        relative_normal_velocity_before_mps,
        relative_normal_velocity_after_mps,
        effective_bat_mass_kg,
    })
}
