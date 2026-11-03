"""Published mass properties, mapped onto the diagnostic reference impact point."""
import math

PRESET = 'kensrud2017_wood'
OUNCE_KG = 0.028349523125  # avoirdupois mass ounce, not fluid ounce
INCH_M = 0.0254


def measured_wood():
    """Table II's single wood bat; I0 is transverse inertia ABOUT THE CM."""
    mass = 29.41 * OUNCE_KG
    inertia = 2370 * OUNCE_KG * INCH_M**2
    impact_from_cm = 6.8 * INCH_M
    return dict(
        preset=PRESET,
        source=dict(
            title='Oblique collisions of baseballs and softballs with a bat',
            authors='Kensrud, Nathan, Smith', year=2017,
            doi='10.1119/1.4982793', table='II, Wood row; p. 505',
            url='https://baseball.physics.illinois.edu/ObliqueCollisionsWSU-AJP.pdf',
            raw=dict(mass_oz=29.41, transverse_cm_inertia_oz_in2=2370,
                     impact_distance_from_cm_in=6.8, radius_at_impact_in=1.16)),
        unit_conversion=dict(ounce_kg=OUNCE_KG, inch_m=INCH_M),
        body_mass_kg=mass, transverse_cm_inertia_kg_m2=inertia,
        cm_offset_from_reference_m=-impact_from_cm,
        gyration_radius_m=math.sqrt(inertia/mass),
        published_radius_at_impact_m=1.16*INCH_M,
        reference_mapping='Saved axial s=0 is mapped to the published impact location, on the barrel side of the CM; positive s points away from the swing pivot. Thus c=-b. Absolute CM/tip coordinates are not inferred.',
        scope='Mass-property counterfactual on saved geometry, not a full reconstruction of the experimental bat. Saved radius, half-length, swing lever and velocities are retained; e=0.5 is a diagnostic assumption, not taken from the table.')


def literature_wood_collision():
    """One complete Table I baseball / Table II wood-bat experiment preset.

    This is independent of Nathan's standard wood bat and positional e curve.
    en=0.5 is a paper modeling assumption; et=0.464 was fitted to the wood data.
    """
    old=measured_wood()
    return dict(preset='kensrud2017_wood_baseball_experiment',
        source_url=old['source']['url'],doi=old['source']['doi'],
        bat_mass_kg=old['body_mass_kg'],transverse_inertia_kg_m2=old['transverse_cm_inertia_kg_m2'],
        axial_inertia_kg_m2=9.1*OUNCE_KG*INCH_M**2,
        impact_from_cm_m=6.8*INCH_M,barrel_radius_m=1.16*INCH_M,
        ball_mass_kg=5.04*OUNCE_KG,ball_radius_m=1.42*INCH_M,ball_inertia_factor=.4,
        normal_e_assumed=.5,wood_fitted_tangent_e=.464,wood_fitted_tangent_e_standard_error=.014,
        experiment_speed_range_ms=[28.,39.],maximum_incident_angle_deg=30.,
        normalization_speed_ms=77*.44704,
        pooled_figure3_spin_slope_rpm_per_deg=179.,pooled_figure4_ev_slope_mph_per_deg=-.65,
        provenance=dict(ball='Table I baseball row; uniform sphere alpha=0.4, Eq.(2)',
            bat='Table II wood row: 29.41 oz, I0=2370 oz in², Iz=9.1 oz in², b=6.8 in, R=1.16 in',
            normal_e='Sec.V A assumes ey=0.5; not a measured wood-bat normal COR',
            tangent_e='Table II wood-baseball fit ex=0.464(14); input derived from experimental spin, not an independent validation target',
            pooled_trends='Fig.3/4 slopes pool baseball impacts across bats; not wood-only raw observations',
            limitations='No raw per-impact observations, pivot trajectory or initial angular speed reconstructed from the PDF. The full 3D cases are local-velocity-conditioned sensitivity cases.'))
