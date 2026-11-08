//! Team standings counts saved with game results. Never aggregate pitch logs on display.
use anyhow::Result;
use rusqlite::Connection;
const COUNTS: &str = "games,wins,losses,draws,runs,runs_allowed,homeruns,steals,hits,at_bats,pitching_outs,errors";

pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(include_str!("../../migrations/upgrades/add_team_stat_cache.sql"))?;
    Ok(())
}
fn fill(conn: &Connection, game: Option<u32>) -> Result<()> {
    // Direct keyed predicates keep incremental saves limited to one game's logs.
    let predicate=|alias:&str| if game.is_some(){format!("{alias}.game_id=?1")}else{"?1 IS NULL".into()};
    let games=if game.is_some(){"g.id=?1"}else{"?1 IS NULL"};
    let sql=format!("WITH
      sides AS (
        SELECT g.id AS game_id,g.away_team_id AS team_id,'Top' AS attack,'Bottom' AS defense,
          COALESCE(g.away_points,0) AS runs,COALESCE(g.home_points,0) AS allowed FROM game g
          WHERE ({games}) AND g.actual_date IS NOT NULL AND g.actual_date!='1900-01-01'
        UNION ALL
        SELECT g.id,g.home_team_id,'Bottom','Top',COALESCE(g.home_points,0),COALESCE(g.away_points,0) FROM game g
          WHERE ({games}) AND g.actual_date IS NOT NULL AND g.actual_date!='1900-01-01'
      ), batting AS (
        SELECT b.game_id,c.inning_tb,
          SUM(b.result='HomeRun') AS hr,
          SUM(b.result IN ('Single','Double','Triple','HomeRun')) AS hits,
          SUM(b.result IN ('Single','Double','Triple','HomeRun','Strikeout','Out','FieldersChoice','DoublePlay')) AS ab
        FROM player_game_batting b JOIN count c ON c.game_id=b.game_id AND c.seq=b.count_seq
        WHERE ({batting}) GROUP BY b.game_id,c.inning_tb
      ), running AS (
        SELECT r.game_id,c.inning_tb,SUM(r.event='BaseSteal' AND r.ruling='Safe') AS sb
        FROM player_game_running r JOIN count c ON c.game_id=r.game_id AND c.seq=r.count_seq
        WHERE ({running}) GROUP BY r.game_id,c.inning_tb
      ), fielding AS (
        SELECT f.game_id,c.inning_tb,COUNT(*) AS errors
        FROM player_game_fielding f JOIN count c ON c.game_id=f.game_id AND c.seq=f.count_seq
        WHERE ({fielding}) AND f.error_type IS NOT NULL AND f.error_type NOT IN ('','None')
        GROUP BY f.game_id,c.inning_tb
      ), half_outs AS (
        SELECT c.game_id,c.inning_tb,c.inning_seq,MIN(3,MAX(c.out)) AS outs
        FROM count c WHERE ({counts}) GROUP BY c.game_id,c.inning_tb,c.inning_seq
      ), pitching AS (
        SELECT game_id,inning_tb,SUM(outs) AS outs FROM half_outs GROUP BY game_id,inning_tb
      )
      INSERT INTO game_team_stat_cache
      SELECT s.game_id,s.team_id,1,s.runs>s.allowed,s.runs<s.allowed,s.runs=s.allowed,
        COALESCE(s.runs,0),COALESCE(s.allowed,0),COALESCE(b.hr,0),COALESCE(r.sb,0),
        COALESCE(b.hits,0),COALESCE(b.ab,0),COALESCE(p.outs,0),COALESCE(f.errors,0)
      FROM sides s
      LEFT JOIN batting b ON b.game_id=s.game_id AND b.inning_tb=s.attack
      LEFT JOIN running r ON r.game_id=s.game_id AND r.inning_tb=s.attack
      LEFT JOIN fielding f ON f.game_id=s.game_id AND f.inning_tb=s.defense
      LEFT JOIN pitching p ON p.game_id=s.game_id AND p.inning_tb=s.defense",
      batting=predicate("b"),running=predicate("r"),fielding=predicate("f"),counts=predicate("c"));
    conn.execute(&sql,[game])?;
    Ok(())
}
fn totals(conn: &Connection, team: Option<u16>) -> Result<()> {
    let filter=if team.is_some(){"team_id=?1"}else{"?1 IS NULL"};
    conn.execute(&format!("DELETE FROM team_stat_totals WHERE {filter}"),[team])?;
    let sums=COUNTS.split(',').map(|c|format!("SUM({c})")).collect::<Vec<_>>().join(",");
    conn.execute(&format!("INSERT INTO team_stat_totals SELECT team_id,{sums} FROM game_team_stat_cache WHERE {filter} GROUP BY team_id"),[team])?;
    Ok(())
}
fn affected(conn: &Connection,game:u32)->Result<Vec<u16>> {
    Ok(conn.prepare("SELECT team_id FROM game_team_stat_cache WHERE game_id=?1")?
        .query_map([game],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?)
}
pub fn refresh_game(conn: &Connection,game:u32)->Result<()> {
    let mut teams=affected(conn,game)?;
    conn.execute("DELETE FROM game_team_stat_cache WHERE game_id=?1",[game])?;
    fill(conn,Some(game))?;
    teams.extend(affected(conn,game)?);teams.sort_unstable();teams.dedup();
    for team in teams {totals(conn,Some(team))?;}
    Ok(())
}
pub fn rebuild(conn:&Connection)->Result<()> {
    clear(conn)?;fill(conn,None)?;totals(conn,None)
}
pub fn clear(conn:&Connection)->Result<()> {
    ensure_schema(conn)?;
    conn.execute_batch("DELETE FROM game_team_stat_cache;DELETE FROM team_stat_totals;")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture()->Connection {
        let conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE game(id INTEGER PRIMARY KEY,away_team_id INTEGER,home_team_id INTEGER,actual_date TEXT,away_points INTEGER,home_points INTEGER);
            CREATE TABLE count(game_id INTEGER,seq INTEGER,inning_seq INTEGER,inning_tb TEXT,out INTEGER);
            CREATE TABLE player_game_batting(game_id INTEGER,count_seq INTEGER,result TEXT);
            CREATE TABLE player_game_running(game_id INTEGER,count_seq INTEGER,event TEXT,ruling TEXT);
            CREATE TABLE player_game_fielding(game_id INTEGER,count_seq INTEGER,error_type TEXT);
            INSERT INTO game VALUES(1,1,2,'2026-04-01',4,2),(2,2,1,'2026-04-02',3,1),(3,1,2,NULL,99,99),(4,1,2,'1900-01-01',99,99);
            INSERT INTO count VALUES(1,1,1,'Top',0),(1,2,1,'Top',1),(1,3,1,'Top',3),(1,4,1,'Bottom',1),(1,5,1,'Bottom',2),(2,1,1,'Top',3),(2,2,1,'Bottom',3);
            INSERT INTO player_game_batting VALUES(1,1,'Single'),(1,2,'HomeRun'),(1,3,'Strikeout'),(1,4,'Out'),(1,5,'Walk'),(2,1,'Double'),(2,2,'Out');
            INSERT INTO player_game_running VALUES(1,1,'BaseSteal','Safe'),(1,1,'BaseSteal','Out'),(1,1,'TagUp','Safe'),(2,1,'BaseSteal','Safe');
            INSERT INTO player_game_fielding VALUES(1,2,'Fumble'),(1,4,'Passed'),(2,1,NULL),(2,2,'None');").unwrap();
        ensure_schema(&conn).unwrap();conn
    }
    #[test]
    fn aggregate_team_sides_terminal_at_bats_steals_errors_and_partial_outs() {
        let conn=fixture();rebuild(&conn).unwrap();
        let row:(i64,i64,i64,i64,i64,i64,i64,i64,i64,i64,i64)=conn.query_row(
            "SELECT games,wins,losses,runs,runs_allowed,homeruns,steals,hits,at_bats,pitching_outs,errors FROM team_stat_totals WHERE team_id=1",[],
            |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?))).unwrap();
        assert_eq!(row,(2,1,1,5,5,1,1,2,4,5,1));
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM game_team_stat_cache",[],|r|r.get::<_,i64>(0)).unwrap(),4);
        let home:(i64,i64,i64)=conn.query_row("SELECT steals,pitching_outs,errors FROM team_stat_totals WHERE team_id=2",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        assert_eq!(home,(1,6,1));
    }
    #[test]
    fn replacing_or_deleting_game_replaces_old_team_contributions() {
        let conn=fixture();rebuild(&conn).unwrap();
        refresh_game(&conn,1).unwrap();refresh_game(&conn,1).unwrap();
        assert_eq!(conn.query_row("SELECT games FROM team_stat_totals WHERE team_id=1",[],|r|r.get::<_,i64>(0)).unwrap(),2);
        conn.execute("UPDATE game SET away_team_id=3 WHERE id=1",[]).unwrap();refresh_game(&conn,1).unwrap();
        assert_eq!(conn.query_row("SELECT homeruns FROM team_stat_totals WHERE team_id=3",[],|r|r.get::<_,i64>(0)).unwrap(),1);
        assert_eq!(conn.query_row("SELECT homeruns FROM team_stat_totals WHERE team_id=1",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        conn.execute("DELETE FROM game WHERE id=1",[]).unwrap();refresh_game(&conn,1).unwrap();
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM team_stat_totals WHERE team_id=3",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert_eq!(conn.query_row("SELECT games FROM team_stat_totals WHERE team_id=2",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    }
    #[test]
    fn failed_team_cache_write_rolls_back_header_and_all_team_counts() {
        let mut conn=fixture();rebuild(&conn).unwrap();
        conn.execute_batch("CREATE TRIGGER reject_team BEFORE INSERT ON team_stat_totals BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
        {
            let tx=conn.transaction().unwrap();tx.execute("UPDATE game SET away_points=10 WHERE id=1",[]).unwrap();
            assert!(refresh_game(&tx,1).is_err());
        }
        assert_eq!(conn.query_row("SELECT away_points FROM game WHERE id=1",[],|r|r.get::<_,i64>(0)).unwrap(),4);
        assert_eq!(conn.query_row("SELECT runs FROM team_stat_totals WHERE team_id=1",[],|r|r.get::<_,i64>(0)).unwrap(),5);
    }
}
