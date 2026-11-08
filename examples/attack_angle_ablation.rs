//! Replay saved contacts with only the batter-type attack-angle contribution varied.
use anyhow::{Context, Result, bail, ensure};
use clap::Parser;
use rusqlite::{Connection, OpenFlags};
use saberbb::domain::{
    resolver::batting_resolver::{
        calculate_collision_spin_from_offsets, calculate_dynamic_attack_angle,
        calculate_trajectory, calculate_vertical_contact_deflection,
    },
    shared::{ball::BattedBall, player::BatterType, stadium::Stadium},
    strategy::batting_strategy::calculate_attack_angle_modifier,
};
use saberbb::repositories::db::FromRow;
use serde::Serialize;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    season: Option<u16>,
    #[arg(long)]
    batter_id: Option<i64>,
}
#[derive(Clone, Debug)]
struct Contact {
    game_id: i64,
    count_seq: i64,
    batter_id: i64,
    stadium_id: i64,
    kind: String,
    ev: f64,
    vla: f64,
    hla: f64,
    distance: f64,
    angle: f64,
    time: f64,
    bat_speed: Option<f64>,
    length: Option<f64>,
    thickness: Option<f64>,
    pitch_spin: Option<f64>,
    pitch_axis: Option<f64>,
    z: Option<f64>,
    bat_angle: Option<f64>,
    base_attack: Option<f64>,
    batter_type: Option<String>,
}
#[derive(Default, Serialize)]
struct Scenario {
    contacts: u64,
    hr: u64,
    mean_vla_deg: f64,
    mean_final_distance_m: f64,
    transitions: BTreeMap<String, u64>,
}
impl Scenario {
    fn add(&mut self, original: &str, ball: &BattedBall) {
        self.contacts += 1;
        self.hr += u64::from(ball.outbound_result.as_ref() == "HomeRun");
        self.mean_vla_deg += (ball.launch_angle - self.mean_vla_deg) / self.contacts as f64;
        self.mean_final_distance_m +=
            (ball.final_position.distance - self.mean_final_distance_m) / self.contacts as f64;
        *self
            .transitions
            .entry(format!("{original}->{}", ball.outbound_result.as_ref()))
            .or_default() += 1;
    }
}
#[derive(Default, Serialize)]
struct Group {
    recorded_contacts: u64,
    recorded_hr: u64,
    comparable_contacts: u64,
    baseline: Scenario,
    half_aggressive_bonus: Scenario,
    no_aggressive_bonus: Scenario,
    no_type_bonus: Scenario,
}
impl Group {
    fn observe(&mut self, c: &Contact) {
        self.recorded_contacts += 1;
        self.recorded_hr += u64::from(c.kind == "HomeRun");
    }
    fn add(&mut self, c: &Contact, balls: &[BattedBall; 4]) {
        self.comparable_contacts += 1;
        self.baseline.add(&c.kind, &balls[0]);
        self.half_aggressive_bonus.add(&c.kind, &balls[1]);
        self.no_aggressive_bonus.add(&c.kind, &balls[2]);
        self.no_type_bonus.add(&c.kind, &balls[3]);
    }
}
fn required(value: Option<f64>) -> Result<f64> {
    value
        .filter(|v| v.is_finite())
        .context("missing_or_nonfinite_input")
}
fn replay(c: &Contact, stadium: &Stadium) -> Result<[BattedBall; 4]> {
    ensure!(
        [c.ev, c.vla, c.hla, c.distance, c.angle, c.time]
            .iter()
            .all(|v| v.is_finite()),
        "nonfinite_saved_trajectory"
    );
    let speed = required(c.bat_speed)?;
    let length = required(c.length)?;
    let thickness = required(c.thickness)?;
    ensure!(
        speed > 0.0 && length >= 0.0 && thickness >= 0.0,
        "invalid_contact_geometry"
    );
    let spin = required(c.pitch_spin)?;
    let axis = required(c.pitch_axis)?;
    ensure!(spin >= 0.0, "invalid_pitch_spin");
    let (spin, axis) = calculate_collision_spin_from_offsets(length, thickness, speed, spin, axis);
    let trajectory = |vla| {
        calculate_trajectory(c.ev, vla, c.hla, spin, axis, stadium)
            .map_err(|_| anyhow::anyhow!("trajectory_failed"))
    };
    let baseline = trajectory(c.vla)?;
    let angle_diff = (baseline.final_position.angle - c.angle + 180.0).rem_euclid(360.0) - 180.0;
    ensure!(
        baseline.outbound_result.as_ref() == c.kind
            && (baseline.final_position.distance - c.distance).abs() < 1e-5
            && angle_diff.abs() < 1e-5
            && (baseline.total_time - c.time).abs() < 1e-6,
        "baseline_reconstruction_mismatch"
    );
    // Recover the actual sampled contribution, not just its nominal 10-degree mean.
    // This inversion assumes the batter's current base attack angle has not changed.
    let kind = c
        .batter_type
        .as_deref()
        .context("missing_batter_type")?
        .parse::<BatterType>()
        .map_err(|_| anyhow::anyhow!("unknown_batter_type"))?;
    let nominal = calculate_attack_angle_modifier(kind);
    let bonus = c.vla
        - calculate_vertical_contact_deflection(required(c.z)?)
        - calculate_dynamic_attack_angle(required(c.base_attack)?, required(c.bat_angle)?);
    if nominal == 0.0 {
        ensure!(bonus.abs() < 1e-7, "type_bonus_reconstruction_mismatch");
    } else {
        ensure!(
            (0.5..=1.5).contains(&(bonus / nominal)),
            "type_bonus_reconstruction_mismatch"
        );
    }
    let bonus = if nominal == 0.0 { 0.0 } else { bonus };
    let aggressive = kind == BatterType::AggressiveFreeSwinger;
    let no_type = if bonus == 0.0 {
        baseline
    } else {
        trajectory(c.vla - bonus)?
    };
    let no_aggressive = if aggressive { no_type } else { baseline };
    let half_aggressive = if aggressive {
        trajectory(c.vla - bonus * 0.5)?
    } else {
        baseline
    };
    Ok([baseline, half_aggressive, no_aggressive, no_type])
}
fn audit(conn: &Connection, args: &Args) -> Result<serde_json::Value> {
    let source_counts = conn.query_row(
        "SELECT COUNT(*),COALESCE(SUM(b.result='HomeRun'),0)
        FROM player_game_batting b JOIN game g ON g.id=b.game_id
        WHERE (?1 IS NULL OR g.season=?1) AND (?2 IS NULL OR b.batter_id=?2)",
        rusqlite::params![args.season, args.batter_id],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    )?;
    let mut stadiums = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT * FROM stadium")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let s = Stadium::from_row(row)?;
        stadiums.insert(i64::from(s.id), s);
    }
    let mut stmt=conn.prepare("SELECT b.game_id,b.count_seq,b.batter_id,g.stadium_id,
        b.outbound_result,b.launch_speed,b.launch_angle,b.final_hla_deg,b.polar_distance,b.polar_angle,b.total_time,
        b.bat_speed,b.length_offset_m,b.thickness_offset_m,p.spin_rate,p.spin_angle,
        b.contact_offset_z_m,b.actual_bat_angle_deg,i.attack_angle,i.batter_type
        FROM player_game_batting b JOIN game g ON g.id=b.game_id
        LEFT JOIN player_game_pitching p ON p.game_id=b.game_id AND p.count_seq=b.count_seq
        LEFT JOIN batter_info i ON i.player_id=b.batter_id
        WHERE b.launch_speed>0 AND (?1 IS NULL OR g.season=?1) AND (?2 IS NULL OR b.batter_id=?2)
        ORDER BY b.game_id,b.count_seq")?;
    let mut rows = stmt.query(rusqlite::params![args.season, args.batter_id])?;
    let mut total = Group::default();
    let mut by_type = BTreeMap::<String, Group>::new();
    let mut by_player = BTreeMap::<i64, Group>::new();
    let mut exclusions = BTreeMap::<String, u64>::new();
    let mut exclusion_examples = Vec::new();
    while let Some(r) = rows.next()? {
        let c = Contact {
            game_id: r.get(0)?,
            count_seq: r.get(1)?,
            batter_id: r.get(2)?,
            stadium_id: r.get(3)?,
            kind: r.get(4)?,
            ev: r.get(5)?,
            vla: r.get(6)?,
            hla: r.get(7)?,
            distance: r.get(8)?,
            angle: r.get(9)?,
            time: r.get(10)?,
            bat_speed: r.get(11)?,
            length: r.get(12)?,
            thickness: r.get(13)?,
            pitch_spin: r.get(14)?,
            pitch_axis: r.get(15)?,
            z: r.get(16)?,
            bat_angle: r.get(17)?,
            base_attack: r.get(18)?,
            batter_type: r.get(19)?,
        };
        let kind = c.batter_type.clone().unwrap_or_else(|| "unknown".into());
        total.observe(&c);
        by_type.entry(kind.clone()).or_default().observe(&c);
        by_player.entry(c.batter_id).or_default().observe(&c);
        let result = stadiums
            .get(&c.stadium_id)
            .context("missing_stadium")
            .and_then(|s| replay(&c, s));
        match result {
            Ok(balls) => {
                total.add(&c, &balls);
                by_type.get_mut(&kind).unwrap().add(&c, &balls);
                by_player.get_mut(&c.batter_id).unwrap().add(&c, &balls);
            }
            Err(e) => {
                let reason = e.to_string();
                *exclusions.entry(reason.clone()).or_default() += 1;
                if exclusion_examples.len() < 20 {
                    exclusion_examples.push(serde_json::json!({"game_id":c.game_id,"count_seq":c.count_seq,"reason":reason}));
                }
            }
        }
    }
    Ok(
        serde_json::json!({"scope":{"season":args.season,"batter_id":args.batter_id},
        "limitations":["Frozen-contact trajectory experiment, not a full-game resimulation; no counterfactual BA or ERA is computed.",
            "EV, HLA, spin, contact geometry, wind and stadium are fixed; only VLA changes.",
            "Bonus is inferred using current batter base attack angle/type, which must match those at recording time; the 0.5–1.5 factor check is not proof of historical identity.",
            "Compare only the matched/comparable pool. Exclusions are not zero-HR outcomes.",
            "Final distance includes rolling or fence termination; it is not unrestricted airborne carry."],
        "source_counts":{"pitch_records":source_counts.0,"raw_hr":source_counts.1},
        "cohort":"Saved records with positive launch_speed; raw_hr includes all records to expose missing-trajectory HRs.",
        "comparison_available":total.comparable_contacts>0,
        "all_positive_ev_contacts_comparable":total.recorded_contacts==total.comparable_contacts,
        "total":total,"by_batter_type":by_type,"by_player":by_player,
        "exclusions":exclusions,"exclusion_examples":exclusion_examples}),
    )
}
fn main() -> Result<()> {
    let args = Args::parse();
    let path = std::env::var("SABERBB_DB").context("SABERBB_DB is not set")?;
    if path.trim().is_empty() {
        bail!("SABERBB_DB is blank");
    }
    let path = PathBuf::from(path);
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(std::time::Duration::from_secs(30))?;
    conn.execute_batch("PRAGMA query_only=ON;BEGIN;")?;
    let result = audit(&conn, &args)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Contact {
        let (spin, axis) = calculate_collision_spin_from_offsets(0.01, 0.01, 35.0, 2400.0, 0.0);
        let ball = calculate_trajectory(47.0, 20.4, 0.0, spin, axis, &Stadium::default()).unwrap();
        Contact {
            game_id: 1,
            count_seq: 1,
            batter_id: 1,
            stadium_id: 1,
            kind: ball.outbound_result.as_ref().into(),
            ev: 47.0,
            vla: 20.4,
            hla: 0.0,
            distance: ball.final_position.distance,
            angle: ball.final_position.angle,
            time: ball.total_time,
            bat_speed: Some(35.0),
            length: Some(0.01),
            thickness: Some(0.01),
            pitch_spin: Some(2400.0),
            pitch_axis: Some(0.0),
            z: Some(0.0),
            bat_angle: Some(30.0),
            base_attack: Some(10.0),
            batter_type: Some("AggressiveFreeSwinger".into()),
        }
    }
    #[test]
    fn database_audit_counts_missing_pitch_records_as_exclusions_without_writes() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE stadium(id,name,foul_pole_distance,center_fence_distance,fence_line,fence_height);
            INSERT INTO stadium VALUES(1,'Test',98.0,120.0,NULL,2.0);
            CREATE TABLE game(id,season,stadium_id);INSERT INTO game VALUES(1,2026,1);
            CREATE TABLE batter_info(player_id,batter_type,attack_angle);
            INSERT INTO batter_info VALUES(1,'AggressiveFreeSwinger',10.0);
            CREATE TABLE player_game_pitching(game_id,count_seq,spin_rate,spin_angle);
            INSERT INTO player_game_pitching VALUES(1,1,2400.0,0.0);
            CREATE TABLE player_game_batting(game_id,count_seq,batter_id,outbound_result,launch_speed,
                launch_angle,final_hla_deg,polar_distance,polar_angle,total_time,bat_speed,
                length_offset_m,thickness_offset_m,contact_offset_z_m,actual_bat_angle_deg,result);").unwrap();
        let c = fixture();
        for seq in [1, 2] {
            conn.execute("INSERT INTO player_game_batting VALUES(1,?1,1,?2,?3,?4,?5,?6,?7,?8,35.0,0.01,0.01,0.0,30.0,?9)",
                rusqlite::params![seq,c.kind,c.ev,c.vla,c.hla,c.distance,c.angle,c.time,if c.kind=="HomeRun" {"HomeRun"}else{"Out"}]).unwrap();
        }
        conn.execute_batch("PRAGMA query_only=ON;").unwrap();
        let changes = conn.total_changes();
        let report = audit(
            &conn,
            &Args {
                season: Some(2026),
                batter_id: Some(1),
            },
        )
        .unwrap();
        assert_eq!(report["source_counts"]["pitch_records"], 2);
        assert_eq!(report["total"]["recorded_contacts"], 2);
        assert_eq!(report["total"]["comparable_contacts"], 1);
        assert_eq!(report["exclusions"]["missing_or_nonfinite_input"], 1);
        assert_eq!(conn.total_changes(), changes);
        let empty = audit(
            &conn,
            &Args {
                season: Some(2027),
                batter_id: None,
            },
        )
        .unwrap();
        assert_eq!(empty["comparison_available"], false);
    }

    #[test]
    fn removes_actual_sampled_bonus_with_ev_and_spin_held_fixed() {
        let c = fixture();
        let balls = replay(&c, &Stadium::default()).unwrap();
        assert!((balls[1].launch_angle - 15.2).abs() < 1e-10);
        assert!((balls[2].launch_angle - 10.0).abs() < 1e-10);
        for b in balls {
            assert_eq!(b.launch_speed, c.ev);
            assert_eq!(b.spin_rate, balls[0].spin_rate);
            assert_eq!(b.spin_angle, balls[0].spin_angle);
        }
    }
    #[test]
    fn mismatched_baseline_or_missing_inputs_cannot_enter_comparison() {
        let mut c = fixture();
        c.distance += 0.01;
        assert!(
            replay(&c, &Stadium::default())
                .unwrap_err()
                .to_string()
                .contains("baseline_reconstruction")
        );
        let mut c = fixture();
        c.pitch_spin = None;
        assert!(replay(&c, &Stadium::default()).is_err());
        let mut c = fixture();
        c.base_attack = Some(-50.0);
        assert!(
            replay(&c, &Stadium::default())
                .unwrap_err()
                .to_string()
                .contains("type_bonus_reconstruction")
        );
    }
    #[test]
    fn aggressive_ablation_preserves_other_types() {
        let mut c = fixture();
        c.batter_type = Some("ClassicAnalyst".into());
        c.base_attack = Some(18.32);
        let balls = replay(&c, &Stadium::default()).unwrap();
        assert_eq!(balls[0].launch_angle, balls[1].launch_angle);
        assert_eq!(balls[0].launch_angle, balls[2].launch_angle);
        assert!((balls[3].launch_angle - 18.32).abs() < 1e-10);
    }
}
