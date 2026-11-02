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
