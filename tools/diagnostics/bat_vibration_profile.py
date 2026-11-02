"""Independent standard-wood-bat template; coarse reading of Nathan (2000) Fig.12."""
import bisect
import math

SOURCE_URL='https://baseball.physics.illinois.edu/AJP-Nov2000.pdf'
REFERENCE_Z_M=.71  # Explicit mapping of saved s=0; not a measured input-bat node.
MASS_KG=.885
CM_Z_M=.564
GYRATION_M=.23
LENGTH_M=.84
# (z from knob in metres, solid full-speed e_eff, dashed half-speed e_eff).
# Rounded visual readout at 0.01 resolution, not author-supplied numerical data.
KNOTS=((.54,.22,.23),(.56,.24,.26),(.58,.28,.30),(.60,.31,.34),
       (.62,.35,.38),(.64,.40,.43),(.66,.45,.47),(.68,.48,.50),
       (.70,.50,.50),(.72,.50,.50),(.74,.49,.49),(.76,.43,.43),
       (.78,.38,.39),(.80,.31,.33),(.82,.24,.27),(.84,.21,.25))


def profile_e(z_m,template='full_speed'):
    if template not in ('full_speed','half_speed') or not math.isfinite(z_m):raise ValueError('invalid_profile_lookup')
    if not KNOTS[0][0]<=z_m<=KNOTS[-1][0]:return None
    column=1 if template=='full_speed' else 2
    j=bisect.bisect_left([p[0] for p in KNOTS],z_m)
    if KNOTS[j][0]==z_m:return KNOTS[j][column]
    left,right=KNOTS[j-1],KNOTS[j]
    return left[column]+(right[column]-left[column])*(z_m-left[0])/(right[0]-left[0])


def apparent_from_e(e,inverse_mass,ball_mass=.145):
    if not all(math.isfinite(x) for x in (e,inverse_mass,ball_mass)) or not 0<=e<=1 or inverse_mass<0 or ball_mass<=0:
        raise ValueError('invalid_cor_conversion')
    r=ball_mass*inverse_mass
    return (e-r)/(1+r)


def e_from_apparent(q,inverse_mass,ball_mass=.145):
    if not all(math.isfinite(x) for x in (q,inverse_mass,ball_mass)) or inverse_mass<0 or ball_mass<=0:
        raise ValueError('invalid_cor_conversion')
    r=ball_mass*inverse_mass;e=q*(1+r)+r
    if not -1e-12<=e<=1+1e-12:raise ValueError('incompatible_apparent_cor')
    return e


def metadata():
    return dict(preset='nathan2000_standard_wood',source_url=SOURCE_URL,
        source_pdf_sha256='637676fb76dfe13631a5d0b6d13f1dcd6960c500c4174145969fba6b49374e8b',
        source_locations='Table I p.984, Table II p.985, Eq.(18) p.984, Fig.12 p.986',
        source_kind='Published simulation on a specified wood bat, supported by low-speed experimental comparisons; NOT measured high-speed COR samples',
        mass_kg=MASS_KG,cm_from_knob_m=CM_Z_M,gyration_m=GYRATION_M,
        transverse_cm_inertia_kg_m2=MASS_KG*GYRATION_M**2,length_m=LENGTH_M,
        barrel_nodes_m=[.68,.72,.74],saved_reference_from_knob_m=REFERENCE_Z_M,
        cm_offset_from_saved_reference_m=CM_Z_M-REFERENCE_Z_M,
        curve_knots=KNOTS,curve_method='Manual 0.01-resolution visual readout, piecewise linear interpolation on [0.54,0.84] m only. Reading uncertainty is not a statistical confidence interval.',
        coefficient='Fig.12 e_eff maps directly to solver e in Eq.(18); it is NOT apparent COR. Never convert it from apparent COR again.',
        source_speed='Full-speed template: ball=40 m/s, bat CM=24 m/s, omega=51 /s. Relative speed at z is 64+51*(z-0.564). Half-speed template uses half this speed.',
        assumptions='Saved s=0 maps to z=0.71 m by explicit convention; fixed saved shape/axis/normal/velocities/lever. Side contacts in curve range only. Both speed curves are templates, not a fit to each saved impact speed or game results. No elastic modes integrated during contact.')
