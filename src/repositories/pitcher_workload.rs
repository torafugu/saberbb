//! Game appearances are the persistent workload history. Rest is applied when reading it.
use crate::domain::shared::{game::GameSchedule, pitcher_fatigue::PitcherFatigue};
use chrono::NaiveDate;
use rusqlite::{Connection, Result, params};
use std::collections::BTreeMap;

pub(super) fn record_game(conn: &Connection, game: u32) -> Result<()> {
    conn.execute_batch(include_str!(
        "../../migrations/ddl/game_pitcher_workload_table.sql"
    ))?;
    let mut appearances = conn.prepare_cached(
        "SELECT p.pitcher_id,COUNT(*),COALESCE(s.stamina,0.0)
         FROM player_game_pitching p LEFT JOIN pitcher_info s ON s.player_id=p.pitcher_id
         WHERE p.game_id=?1 GROUP BY p.pitcher_id",
    )?;
    let appearances = appearances.query_map([game], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, u32>(1)?,
            row.get::<_, f64>(2)?,
        ))
    })?;
    conn.execute("DELETE FROM game_pitcher_workload WHERE game_id=?1", [game])?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO game_pitcher_workload(game_id,player_id,pitches,reference_pitches)
         VALUES (?1,?2,?3,?4)",
    )?;
    for appearance in appearances {
        let (player, pitches, stamina) = appearance?;
        insert.execute(params![
            game,
            player,
            pitches,
            PitcherFatigue::reference_pitches(stamina)
        ])?;
    }
    Ok(())
}

pub(super) fn load_game(conn: &Connection, game: u32) -> Result<BTreeMap<i64, PitcherFatigue>> {
    let mut fatigue = BTreeMap::new();
    if !table_exists(conn)? {
        return Ok(fatigue);
    }

    // Sum residual loads by appearance so each can use its saved stamina.
    let mut query = conn.prepare_cached(
        "SELECT w.player_id,w.pitches,w.reference_pitches,g.actual_date,
                p.pitches,earlier.actual_date,p.reference_pitches
         FROM game_pitcher_workload w JOIN game g ON g.id=w.game_id
         LEFT JOIN game_pitcher_workload p ON p.player_id=w.player_id
         LEFT JOIN game earlier ON earlier.id=p.game_id
          AND (earlier.actual_date,COALESCE(earlier.seq,0),earlier.id)
            < (g.actual_date,COALESCE(g.seq,0),g.id)
         WHERE w.game_id=?1
         ORDER BY w.player_id,earlier.actual_date,COALESCE(earlier.seq,0),earlier.id",
    )?;
    let mut rows = query.query([game])?;
    while let Some(row) = rows.next()? {
        let (player, pitches, reference_pitches) = (row.get(0)?, row.get(1)?, row.get(2)?);
        let state = fatigue.entry(player).or_insert(PitcherFatigue {
            entering_load: 0.0,
            pitches,
            reference_pitches,
        });
        if let Some(previous_date) = row.get::<_, Option<NaiveDate>>(5)? {
            let current_date: NaiveDate = row.get(3)?;
            let earlier_pitches = row.get(4)?;
            state.entering_load += PitcherFatigue::remaining_load(
                earlier_pitches,
                (current_date - previous_date).num_days(),
                row.get(6)?,
                reference_pitches,
            );
        }
    }
    Ok(fatigue)
}

/// Read the committed workload for both rosters immediately before the game.
pub(super) fn load_before_game(
    conn: &Connection,
    game: &GameSchedule,
) -> Result<BTreeMap<i64, f64>> {
    let mut loads = BTreeMap::new();
    if !table_exists(conn)? {
        return Ok(loads);
    }
    let mut query = conn.prepare_cached(
        "SELECT w.player_id,w.pitches,g.actual_date,w.reference_pitches,COALESCE(s.stamina,0.0)
         FROM player_info p JOIN game_pitcher_workload w ON w.player_id=p.id
         JOIN game g ON g.id=w.game_id
         LEFT JOIN pitcher_info s ON s.player_id=w.player_id
         WHERE p.team_id IN (?1,?2) AND (g.actual_date,COALESCE(g.seq,0),g.id)<(?3,?4,?5)
         ORDER BY w.player_id,g.actual_date,COALESCE(g.seq,0),g.id",
    )?;
    let mut rows = query.query(params![
        game.away_team.id,
        game.home_team.id,
        game.planned_date,
        game.seq,
        game.id
    ])?;
    while let Some(row) = rows.next()? {
        let player = row.get(0)?;
        let date: NaiveDate = row.get(2)?;
        let days = (game.planned_date - date).num_days();
        let current_reference = PitcherFatigue::reference_pitches(row.get(4)?);
        *loads.entry(player).or_default() +=
            PitcherFatigue::remaining_load(row.get(1)?, days, row.get(3)?, current_reference);
    }
    Ok(loads)
}

pub fn clear(conn: &Connection) -> Result<()> {
    if table_exists(conn)? {
        conn.execute("DELETE FROM game_pitcher_workload", [])?;
    }
    Ok(())
}

fn table_exists(conn: &Connection) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='game_pitcher_workload')",
        [],
        |row| row.get(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::{game::GameType, stadium::Stadium, team::Team};

    #[test]
    fn workload_uses_committed_pitches_and_game_dates() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE game(id INTEGER PRIMARY KEY, seq INTEGER, actual_date TEXT);
             CREATE TABLE player_game_pitching(game_id INTEGER, pitcher_id INTEGER);
             CREATE TABLE pitcher_info(player_id INTEGER PRIMARY KEY, stamina REAL);
             CREATE TABLE player_info(id INTEGER PRIMARY KEY, team_id INTEGER);
             INSERT INTO player_info VALUES (7,1),(8,3);
             INSERT INTO pitcher_info VALUES (7,0.0),(8,1.0);
             INSERT INTO game VALUES
                (1,1,'2026-04-01'),(2,1,'2026-04-02'),(3,2,'2026-04-02'),
                (4,1,'2026-04-04'),(5,1,'2026-03-31');
             WITH RECURSIVE pitches(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM pitches WHERE n<100)
             INSERT INTO player_game_pitching
               SELECT 1,7 FROM pitches
               UNION ALL SELECT 2,7 FROM pitches WHERE n<=30
               UNION ALL SELECT 3,7 FROM pitches WHERE n<=10
               UNION ALL SELECT 4,7 FROM pitches WHERE n<=20
               UNION ALL SELECT 2,8 FROM pitches WHERE n<=5
               UNION ALL SELECT 5,7 FROM pitches;",
        )
        .unwrap();

        // Older games have no workload records until the feature is enabled.
        assert!(load_game(&conn, 5).unwrap().is_empty());
        for game in 1..=4 {
            record_game(&conn, game).unwrap();
        }
        record_game(&conn, 2).unwrap();
        assert!(load_game(&conn, 5).unwrap().is_empty());
        let first = load_game(&conn, 1).unwrap();
        assert_eq!(first[&7].entering_load, 0.0);
        assert_eq!(first[&7].leaving_ratio(), 1.0);
        let next_day = load_game(&conn, 2).unwrap();
        assert_eq!(next_day[&7].entering_load, 50.0);
        assert_eq!(next_day[&7].leaving_ratio(), 0.8);
        assert_eq!(next_day[&8].entering_load, 0.0);
        assert_eq!(next_day[&8].reference_pitches, 115.0);
        assert_eq!(load_game(&conn, 3).unwrap()[&7].entering_load, 80.0);
        assert_eq!(load_game(&conn, 4).unwrap()[&7].entering_load, 22.5);

        let schedule = GameSchedule {
            id: 3,
            season: 2026,
            round_seq: 1,
            seq: 2,
            planned_date: "2026-04-02".parse().unwrap(),
            away_team: Team::min(1, "Away"),
            home_team: Team::min(2, "Home"),
            stadium: Stadium::default(),
            game_type: GameType::Regular,
            max_inning: 12,
            base_four_seam_speed: 40.833,
        };
        // Same-day earlier games count, the game itself, later games and other teams do not.
        assert_eq!(
            load_before_game(&conn, &schedule).unwrap(),
            BTreeMap::from([(7, 80.0)])
        );

        let tx = conn.transaction().unwrap();
        tx.execute("DELETE FROM player_game_pitching WHERE game_id=2", [])
            .unwrap();
        record_game(&tx, 2).unwrap();
        tx.rollback().unwrap();
        assert_eq!(load_game(&conn, 4).unwrap()[&7].entering_load, 22.5);
        clear(&conn).unwrap();
        assert!(load_game(&conn, 4).unwrap().is_empty());
    }

    #[test]
    fn stamina_changes_pitch_cost_and_recovery_for_selection_and_replay() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE game(id INTEGER PRIMARY KEY, seq INTEGER, actual_date TEXT);
             CREATE TABLE player_game_pitching(game_id INTEGER, pitcher_id INTEGER);
             CREATE TABLE pitcher_info(player_id INTEGER PRIMARY KEY, stamina REAL);
             CREATE TABLE player_info(id INTEGER PRIMARY KEY, team_id INTEGER);
             INSERT INTO player_info VALUES (7,1),(8,2);
             INSERT INTO pitcher_info VALUES (7,1.0),(8,-1.0);
             INSERT INTO game VALUES (1,1,'2026-04-01'),(2,1,'2026-04-02');
             WITH RECURSIVE pitches(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM pitches WHERE n<80)
             INSERT INTO player_game_pitching
                 SELECT 1,7 FROM pitches UNION ALL SELECT 1,8 FROM pitches
                 UNION ALL SELECT 2,7 UNION ALL SELECT 2,8;",
        )
        .unwrap();
        record_game(&conn, 1).unwrap();
        let first = load_game(&conn, 1).unwrap();
        assert_eq!(first[&7].leaving_ratio(), 80.0 / 115.0);
        assert_eq!(first[&8].leaving_ratio(), 80.0 / 85.0);

        let schedule = GameSchedule {
            id: 2,
            season: 2026,
            round_seq: 1,
            seq: 1,
            planned_date: "2026-04-02".parse().unwrap(),
            away_team: Team::min(1, "Away"),
            home_team: Team::min(2, "Home"),
            stadium: Stadium::default(),
            game_type: GameType::Regular,
            max_inning: 12,
            base_four_seam_speed: 40.833,
        };
        let recovered_loads = load_before_game(&conn, &schedule).unwrap();
        assert!((recovered_loads[&7] / 80.0 - 0.451).abs() < 0.001);
        assert!((recovered_loads[&8] / 80.0 - 0.555).abs() < 0.001);

        // A new stamina setting applies to new pitches. Historical fatigue
        // keeps the cost and recovery rate saved when those pitches were thrown.
        conn.execute("UPDATE pitcher_info SET stamina=0.0 WHERE player_id=7", [])
            .unwrap();
        let loads = load_before_game(&conn, &schedule).unwrap();
        assert!((loads[&7] / 100.0 - recovered_loads[&7] / 115.0).abs() < 1e-12);
        assert_eq!(loads[&8], recovered_loads[&8]);

        record_game(&conn, 2).unwrap();
        let next_day = load_game(&conn, 2).unwrap();
        assert_eq!(next_day[&7].entering_load, loads[&7]);
        assert_eq!(next_day[&8].entering_load, loads[&8]);
        let new_pitch_cost = next_day[&7].leaving_ratio() - next_day[&7].entering_ratio();
        assert!((new_pitch_cost - 0.01).abs() < 1e-12);
        assert_eq!(load_game(&conn, 1).unwrap(), first);
    }
}
