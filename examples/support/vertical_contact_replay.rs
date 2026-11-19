//! Shared trajectory replay for a world-vertical contact displacement.
use anyhow::Result;
use saberbb::domain::{
    resolver::batting_resolver::{
        SwingContactResult, calculate_collision_spin_from_offsets,
        calculate_launch_speed_with_power, calculate_trajectory,
        calculate_vertical_contact_deflection, contact_geometry_is_miss, contact_normal_hla_deg,
        project_contact_offsets, wrap_hla_deg,
    },
    shared::{ball::BattedBall, stadium::Stadium},
};

pub(super) struct Inputs {
    pub(super) x: f64,
    pub(super) z: f64,
    pub(super) tilt: f64,
    pub(super) depth: f64,
    pub(super) pitch_speed: f64,
    pub(super) pitch_spin: f64,
    pub(super) pitch_axis: f64,
    pub(super) bat_speed: f64,
    pub(super) power: f64,
    pub(super) handedness: f64,
    pub(super) face_deg: f64,
    pub(super) attack: f64,
}

impl Inputs {
    pub(super) fn launch_direction(&self, signed_thickness: f64) -> f64 {
        wrap_hla_deg(
            (self.face_deg + contact_normal_hla_deg(signed_thickness, self.tilt)) * self.handedness,
        )
    }

    pub(super) fn flight(&self, z: f64, stadium: &Stadium) -> Result<Option<BattedBall>> {
        let (length, thickness) = project_contact_offsets(self.x, z, self.tilt);
        if contact_geometry_is_miss(length, thickness) {
            return Ok(None);
        }
        let contact = SwingContactResult {
            length_offset_m: length.abs(),
            thickness_offset_m: thickness.abs(),
            timing_impact_x_m: self.depth,
            ..SwingContactResult::default()
        };
        let ev = calculate_launch_speed_with_power(
            &contact,
            self.pitch_speed,
            self.bat_speed,
            self.power,
        );
        let (spin, axis) = calculate_collision_spin_from_offsets(
            length.abs(),
            thickness.abs(),
            self.bat_speed,
            self.pitch_spin,
            self.pitch_axis,
        );
        let vla = self.attack + calculate_vertical_contact_deflection(z);
        let hla = self.launch_direction(thickness);
        calculate_trajectory(ev, vla, hla, spin, axis, stadium)
            .map(Some)
            .map_err(|_| anyhow::anyhow!("trajectory_failed"))
    }
}
