//! Read-only schedule preflight; optional explicit scheduling uses the normal scheduler.
use anyhow::{Result,ensure,bail};
use rusqlite::{Connection,OpenFlags,params};
use serde::{Serialize,Deserialize};
use std::path::Path;

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct PlannedRound { pub season:u16,pub round_seq:u16,pub game_ids:Vec<u32> }
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct ScheduleStatus {
    pub current_season:u16,pub current_round_seq:u16,pub scheduled_season:u16,
    pub pending_rounds:Vec<PlannedRound>,pub past_pending_games:u64,
}
pub fn inspect(conn:&Connection)->Result<ScheduleStatus> {
    let rows=conn.prepare("SELECT current_season,current_round_seq,scheduled_season FROM game_season")?
        .query_map([],|r|Ok((r.get::<_,u16>(0)?,r.get::<_,u16>(1)?,r.get::<_,u16>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(rows.len()==1,"game_season must contain exactly one cursor row");
    let (season,round,scheduled)=rows[0];let mut pending:Vec<PlannedRound>=Vec::new();
    for row in conn.prepare("SELECT season,round_seq,id FROM game WHERE actual_date IS NULL AND season>=?1 ORDER BY season,round_seq,id")?
        .query_map([season],|r|Ok((r.get::<_,u16>(0)?,r.get::<_,u16>(1)?,r.get::<_,u32>(2)?)))? {
        let (s,r,id)=row?;
        if pending.last().is_none_or(|p|p.season!=s||p.round_seq!=r) { pending.push(PlannedRound {season:s,round_seq:r,game_ids:Vec::new()}); }
        pending.last_mut().unwrap().game_ids.push(id);
    }
    let past:i64=conn.query_row("SELECT COUNT(*) FROM game WHERE actual_date IS NULL AND season<?1",[season],|r|r.get(0))?;
    Ok(ScheduleStatus { current_season:season,current_round_seq:round,scheduled_season:scheduled,pending_rounds:pending,past_pending_games:past as u64 })
}
pub fn read_status(path:&Path)->Result<ScheduleStatus> {
    inspect(&Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_ONLY)?)
}
pub fn plan(status:&ScheduleStatus,rounds:u32,exclude:u32,allow_align:bool)->Result<Vec<PlannedRound>> {
    ensure!(rounds>0,"rounds must be positive");
    ensure!(status.past_pending_games==0,"unprocessed games remain before current_season; inspect the season cursor before preparing a holdout");
    ensure!(status.pending_rounds.len()>=rounds as usize,
        "requested {rounds} rounds but only {} pending rounds are scheduled; use run --prepare-schedule to create missing future seasons",status.pending_rounds.len());
    let result=status.pending_rounds[..rounds as usize].to_vec();
    ensure!(result.iter().flat_map(|r|&r.game_ids).all(|id|*id>exclude),"pending games overlap the excluded development/parity IDs");
    if !allow_align {
        let mut expected=(status.current_season,status.current_round_seq);
        for p in &result {
            ensure!((p.season,p.round_seq)==expected,"the cursor or season boundary does not match the pending schedule; use run --prepare-schedule to align the declared round plan");
            expected=(p.season,p.round_seq.checked_add(1).ok_or_else(||anyhow::anyhow!("round cursor overflow"))?);
        }
    }
    Ok(result)
}
pub fn prepare(path:&Path,rounds:u32,exclude:u32,allow_schedule:bool,mut schedule_next:impl FnMut()->Result<()>)->Result<(ScheduleStatus,Vec<PlannedRound>)> {
    ensure!(rounds>0,"rounds must be positive");
    let mut status=read_status(path)?;
    if allow_schedule {
        ensure!(status.past_pending_games==0,"unprocessed past-season games require cursor inspection");
        // Refuse exclusions before writing any additional schedule rows.
        ensure!(status.pending_rounds.iter().take(rounds as usize).flat_map(|r|&r.game_ids).all(|id|*id>exclude),"pending games overlap excluded IDs");
        for _ in 0..100 {
            if status.pending_rounds.len()>=rounds as usize { break; }
            ensure!(status.scheduled_season>=status.current_season.saturating_sub(1),"scheduled_season is behind the active season");
            let next=status.scheduled_season.checked_add(1).ok_or_else(||anyhow::anyhow!("scheduled season overflow"))?;
            let conn=Connection::open_with_flags(path,OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            let existing:i64=conn.query_row("SELECT COUNT(*) FROM game WHERE season=?1",[next],|r|r.get(0))?;
            ensure!(existing==0,"season {next} already has schedule rows but scheduled_season is stale; refusing duplicate schedules");
            let before=status.pending_rounds.len();
            schedule_next()?;
            status=read_status(path)?;
            ensure!(status.pending_rounds.len()>before,"scheduling produced no additional pending rounds; inspect leagues and teams");
        }
        if status.pending_rounds.len()<rounds as usize { bail!("schedule preparation limit reached"); }
    }
    let result=plan(&status,rounds,exclude,allow_schedule)?;Ok((status,result))
}
pub fn align_cursor(conn:&mut Connection,expected:(u16,u16),planned:&PlannedRound)->Result<()> {
    let tx=conn.transaction()?;
    let rows=tx.execute("UPDATE game_season SET current_season=?1,current_round_seq=?2 WHERE current_season=?3 AND current_round_seq=?4",
        params![planned.season,planned.round_seq,expected.0,expected.1])?;
    ensure!(rows==1,"season cursor changed during the run; refusing to overwrite another process's progress");
    tx.commit()?;Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TempDir(std::path::PathBuf);
    impl Drop for TempDir { fn drop(&mut self) { let _=std::fs::remove_dir_all(&self.0); } }
    fn database()->(TempDir,std::path::PathBuf,Connection) {
        let id=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir=TempDir(std::env::temp_dir().join(format!("holdout-schedule-{}-{id}",std::process::id())));
        std::fs::create_dir(&dir.0).unwrap();let path=dir.0.join("schedule.db");
        let conn=Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE game_season(current_season INTEGER,current_round_seq INTEGER,scheduled_season INTEGER); INSERT INTO game_season VALUES(2026,47,2026); CREATE TABLE game(id INTEGER PRIMARY KEY,season INTEGER,round_seq INTEGER,actual_date TEXT);").unwrap();
        (dir,path,conn)
    }
    #[test]
    fn exhausted_season_is_detected_before_any_preparation() {
        let (_dir,path,conn)=database();
        let before=std::fs::read(&path).unwrap();
        assert!(prepare(&path,40,0,false,||panic!("must not schedule implicitly")).unwrap_err().to_string().contains("only 0 pending"));
        assert_eq!(std::fs::read(&path).unwrap(),before);
        assert_eq!(inspect(&conn).unwrap().current_round_seq,47);
        let missing=path.with_file_name("missing.db");assert!(read_status(&missing).is_err());assert!(!missing.exists());
    }
    #[test]
    fn explicit_preparation_creates_missing_season_and_aligns_without_duplicates() {
        let (_dir,path,mut conn)=database();let mut calls=0;
        let (status,rounds)=prepare(&path,2,100,true,||{
            calls+=1;conn.execute_batch("INSERT INTO game VALUES(101,2027,1,NULL),(102,2027,2,NULL); UPDATE game_season SET scheduled_season=2027;")?;Ok(())
        }).unwrap();
        assert_eq!(calls,1);assert_eq!(rounds.len(),2);
        assert_eq!(inspect(&conn).unwrap().current_season,2026);
        align_cursor(&mut conn,(status.current_season,status.current_round_seq),&rounds[0]).unwrap();
        assert_eq!(inspect(&conn).unwrap().current_season,2027);
        assert!(align_cursor(&mut conn,(2026,47),&rounds[1]).is_err());
        prepare(&path,2,100,true,||panic!("already enough pending games")).unwrap();
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM game",[],|r|r.get::<_,i64>(0)).unwrap(),2);
    }
    #[test]
    fn season_boundary_and_stale_round_require_explicit_alignment() {
        let (_dir,_path,conn)=database();
        conn.execute_batch("INSERT INTO game VALUES(101,2026,46,NULL),(102,2027,1,NULL); UPDATE game_season SET scheduled_season=2027;").unwrap();
        let status=inspect(&conn).unwrap();assert!(plan(&status,2,100,false).is_err());
        let rounds=plan(&status,2,100,true).unwrap();
        assert_eq!((rounds[0].season,rounds[0].round_seq),(2026,46));
        assert_eq!((rounds[1].season,rounds[1].round_seq),(2027,1));
    }
    #[test]
    fn ambiguous_past_games_exclusions_and_stale_scheduled_pointer_refuse_writes() {
        let (_dir,path,conn)=database();
        conn.execute_batch("INSERT INTO game VALUES(1,2026,47,NULL);").unwrap();
        assert!(prepare(&path,2,1,true,||panic!("excluded games must fail before scheduling")).is_err());
        conn.execute_batch("DELETE FROM game; INSERT INTO game VALUES(2,2025,1,NULL);").unwrap();
        assert!(prepare(&path,2,0,true,||panic!("past games must fail before scheduling")).is_err());
        conn.execute_batch("DELETE FROM game; INSERT INTO game VALUES(3,2027,1,NULL);").unwrap();
        assert!(prepare(&path,2,0,true,||panic!("existing season must not be scheduled twice")).unwrap_err().to_string().contains("refusing duplicate"));
    }
}
