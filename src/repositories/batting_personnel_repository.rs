//! Persistence for player personality and independently identified managers.
use anyhow::{Result,ensure};
use rusqlite::{Connection,OptionalExtension,params};
use crate::domain::shared::batting_personnel::{BatterCharacter,Manager};

fn exists(conn:&Connection,table:&str)->Result<bool> {
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",[table],|r|r.get(0))?)
}
pub fn load_character(conn:&Connection,player_id:i64)->Result<BatterCharacter> {
    if !exists(conn,"player_batting_character")? {return Ok(BatterCharacter::default());}
    let raw:Option<String>=conn.query_row("SELECT character_json FROM player_batting_character WHERE player_id=?1",[player_id],|r|r.get(0)).optional()?;
    Ok(match raw {Some(s)=>serde_json::from_str(&s)?,None=>BatterCharacter::default()})
}
pub fn save_character(conn:&Connection,player_id:i64,character:BatterCharacter)->Result<()> {
    ensure!(player_id>0,"invalid player ID");
    if !exists(conn,"player_batting_character")? {
        ensure!(character==BatterCharacter::default(),"apply batting personnel migration before saving non-default character");return Ok(());
    }
    let known:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM player_info WHERE id=?1)",[player_id],|r|r.get(0))?;ensure!(known,"unknown player ID");
    conn.execute("INSERT INTO player_batting_character(player_id,character_json) VALUES(?1,?2) ON CONFLICT(player_id) DO UPDATE SET character_json=excluded.character_json",params![player_id,serde_json::to_string(&character)?])?;Ok(())
}
pub fn load_team_manager(conn:&Connection,team_id:u16)->Result<Option<Manager>> {
    let people=exists(conn,"manager")?;let assignments=exists(conn,"team_manager")?;
    ensure!(people==assignments,"incomplete manager schema; apply batting personnel migration");
    if !people {return Ok(None);}
    let assigned:Option<i64>=conn.query_row("SELECT manager_id FROM team_manager WHERE team_id=?1",[team_id],|r|r.get(0)).optional()?;
    let Some(id)=assigned else{return Ok(None);};
    let (name,policy):(String,String)=conn.query_row("SELECT name,batting_policy FROM manager WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let manager=Manager {id,name,batting_policy:serde_json::from_value(serde_json::Value::String(policy))?};
    manager.validate().map_err(anyhow::Error::msg)?;Ok(Some(manager))
}
pub fn save_manager(conn:&mut Connection,manager:&Manager,team_id:Option<u16>)->Result<()> {
    manager.validate().map_err(anyhow::Error::msg)?;
    let tx=conn.transaction()?;
    if let Some(team)=team_id {
        let known:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM team WHERE id=?1)",[team],|r|r.get(0))?;ensure!(known,"unknown team ID");
    }
    let policy=serde_json::to_value(manager.batting_policy)?.as_str().unwrap().to_owned();
    tx.execute("INSERT INTO manager(id,name,batting_policy) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET name=excluded.name,batting_policy=excluded.batting_policy",params![manager.id,manager.name,policy])?;
    if let Some(team)=team_id {
        tx.execute("DELETE FROM team_manager WHERE manager_id=?1",[manager.id])?;
        tx.execute("INSERT INTO team_manager(team_id,manager_id) VALUES(?1,?2) ON CONFLICT(team_id) DO UPDATE SET manager_id=excluded.manager_id",params![team,manager.id])?;
    }
    tx.commit()?;Ok(())
}
pub fn migrate(conn:&Connection)->Result<()> {conn.execute_batch(include_str!("../../migrations/upgrades/add_batting_personnel.sql"))?;Ok(())}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::batting_personnel::ManagerBattingPolicy;
    #[test]
    fn migration_persistence_transfer_and_rollback_preserve_people() {
        let mut conn=Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE player_info(id INTEGER PRIMARY KEY);INSERT INTO player_info VALUES(7);CREATE TABLE team(id INTEGER PRIMARY KEY);INSERT INTO team VALUES(1),(2);").unwrap();
        assert_eq!(load_character(&conn,7).unwrap(),BatterCharacter::Balanced);assert!(load_team_manager(&conn,1).unwrap().is_none());
        migrate(&conn).unwrap();save_character(&conn,7,BatterCharacter::Adaptive).unwrap();
        let manager=Manager {id:50,name:"Independent manager".into(),batting_policy:ManagerBattingPolicy::Situational};
        save_manager(&mut conn,&manager,Some(1)).unwrap();migrate(&conn).unwrap();
        assert_eq!(load_character(&conn,7).unwrap(),BatterCharacter::Adaptive);assert_eq!(load_team_manager(&conn,1).unwrap(),Some(manager.clone()));
        save_manager(&mut conn,&manager,Some(2)).unwrap();assert!(load_team_manager(&conn,1).unwrap().is_none());
        assert_eq!(load_team_manager(&conn,2).unwrap(),Some(manager.clone()));
        let bad=Manager {name:"Must not persist".into(),..manager.clone()};assert!(save_manager(&mut conn,&bad,Some(999)).is_err());
        assert_eq!(load_team_manager(&conn,2).unwrap(),Some(manager));
        assert!(save_character(&conn,99,BatterCharacter::Adaptive).is_err());
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM manager",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    }
}
