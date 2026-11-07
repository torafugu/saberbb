use crate::domain::shared::batting_personnel::{Manager, ManagerBattingPolicy};
use crate::domain::shared::player::FullName;
use crate::repositories::db::{DbClient, SqlDb};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, TransactionBehavior, params};

pub trait ManagerRepository {
    fn unmanaged_team_ids(&self) -> Result<Vec<u16>>;
    fn random_name(&self, language: &str) -> Result<FullName>;
    fn create_and_assign(
        &mut self, team_id: u16, name: &str, policy: ManagerBattingPolicy,
    ) -> Result<Option<Manager>>;
}

pub struct SqlManagerRepository {
    db_client: DbClient,
}

impl SqlManagerRepository {
    pub fn new() -> Result<Self> {
        Ok(Self { db_client: DbClient { db: SqlDb::new()? } })
    }
}

impl ManagerRepository for SqlManagerRepository {
    fn unmanaged_team_ids(&self) -> Result<Vec<u16>> {
        let conn = self.db_client.get_conn()?;
        unmanaged_team_ids(&conn)
            .context("load manager vacancies; apply migrations/upgrades/add_batting_personnel.sql to existing databases")
    }

    fn random_name(&self, language: &str) -> Result<FullName> {
        let conn = self.db_client.get_conn()?;
        Ok(crate::repositories::sql_helper::player_helper::random_name(&conn, language)?)
    }

    fn create_and_assign(
        &mut self, team_id: u16, name: &str, policy: ManagerBattingPolicy,
    ) -> Result<Option<Manager>> {
        let mut conn = self.db_client.get_conn()?;
        create_and_assign(&mut conn, team_id, name, policy)
    }
}

fn unmanaged_team_ids(conn: &Connection) -> Result<Vec<u16>> {
    let mut stmt = conn.prepare(
        "SELECT t.id FROM team t
         WHERE NOT EXISTS (SELECT 1 FROM team_manager tm WHERE tm.team_id=t.id)
         ORDER BY t.id",
    )?;
    Ok(stmt.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn create_and_assign(
    conn: &mut Connection, team_id: u16, name: &str, policy: ManagerBattingPolicy,
) -> Result<Option<Manager>> {
    ensure!(!name.trim().is_empty(), "manager name must not be empty");
    // Lock before checking the vacancy so another generator cannot replace it.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let known: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM team WHERE id=?1)", [team_id], |row| row.get(0),
    )?;
    ensure!(known, "unknown team ID: {team_id}");
    let occupied: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM team_manager WHERE team_id=?1)",
        [team_id], |row| row.get(0),
    )?;
    if occupied {
        return Ok(None);
    }
    let policy_text = serde_json::to_value(policy)?;
    tx.execute(
        "INSERT INTO manager(name,batting_policy) VALUES(?1,?2)",
        params![name, policy_text.as_str().unwrap()],
    )?;
    let manager = Manager { id: tx.last_insert_rowid(), name: name.to_owned(), batting_policy: policy };
    manager.validate().map_err(anyhow::Error::msg)?;
    tx.execute(
        "INSERT INTO team_manager(team_id,manager_id) VALUES(?1,?2)",
        params![team_id, manager.id],
    )?;
    tx.commit()?;
    Ok(Some(manager))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::manager_service::ManagerService;
    use crate::domain::random_provider::RealRng;
    use crate::repositories::batting_personnel_repository::{load_team_manager, migrate, save_manager};

    struct MemoryRepo { conn: Connection }

    impl ManagerRepository for MemoryRepo {
        fn unmanaged_team_ids(&self) -> Result<Vec<u16>> { unmanaged_team_ids(&self.conn) }
        fn random_name(&self, language: &str) -> Result<FullName> {
            Ok(crate::repositories::sql_helper::player_helper::random_name(&self.conn, language)?)
        }
        fn create_and_assign(&mut self, team: u16, name: &str, policy: ManagerBattingPolicy) -> Result<Option<Manager>> {
            create_and_assign(&mut self.conn, team, name, policy)
        }
    }

    fn database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE team(id INTEGER PRIMARY KEY);
             INSERT INTO team VALUES(1),(2),(3);
             CREATE TABLE player_info(id INTEGER PRIMARY KEY);
             CREATE TABLE first_names(name TEXT, country TEXT, gender TEXT);
             CREATE TABLE last_names(name TEXT, country TEXT);
             INSERT INTO first_names VALUES('太郎','jp','M'),('John','us','M');
             INSERT INTO last_names VALUES('山田','jp'),('Smith','us');",
        ).unwrap();
        migrate(&conn).unwrap();
        conn
    }

    fn count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM manager", [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn manager_generation_fills_all_vacancies_and_preserves_existing_people() {
        let mut conn = database();
        let existing = Manager { id: 50, name: "Existing manager".into(), batting_policy: ManagerBattingPolicy::Situational };
        save_manager(&mut conn, &existing, Some(2)).unwrap();
        let free = Manager { id: 60, name: "Unassigned manager".into(), batting_policy: ManagerBattingPolicy::SeekLineDrive };
        save_manager(&mut conn, &free, None).unwrap();
        let mut service = ManagerService { repo: MemoryRepo { conn } };
        let mut rng = RealRng::from_seed(7);
        assert_eq!(service.generate_missing_managers("jp", &mut rng).unwrap(), 2);
        let conn = &service.repo.conn;
        assert_eq!(load_team_manager(conn, 2).unwrap(), Some(existing.clone()));
        let first = load_team_manager(conn, 1).unwrap().unwrap();
        let third = load_team_manager(conn, 3).unwrap().unwrap();
        assert_eq!(first.name, "山田 太郎");
        assert_eq!(third.name, "山田 太郎");
        assert!(first.id > free.id && third.id > first.id);
        assert_eq!(count(conn), 4);
        assert_eq!(service.generate_missing_managers("us", &mut rng).unwrap(), 0);
        assert_eq!(count(&service.repo.conn), 4);
        assert_eq!(load_team_manager(&service.repo.conn, 1).unwrap(), Some(first));
        assert_eq!(load_team_manager(&service.repo.conn, 2).unwrap(), Some(existing));
        assert_eq!(load_team_manager(&service.repo.conn, 3).unwrap(), Some(third));
    }

    #[test]
    fn manager_generation_uses_english_name_order_and_handles_no_teams() {
        let mut service = ManagerService { repo: MemoryRepo { conn: database() } };
        assert_eq!(service.generate_missing_managers("us", &mut RealRng::from_seed(42)).unwrap(), 3);
        assert_eq!(load_team_manager(&service.repo.conn, 1).unwrap().unwrap().name, "John Smith");
        service.repo.conn.execute("DELETE FROM team_manager", []).unwrap();
        service.repo.conn.execute("DELETE FROM team", []).unwrap();
        service.repo.conn.execute("DELETE FROM first_names", []).unwrap();
        assert_eq!(service.generate_missing_managers("jp", &mut RealRng::from_seed(42)).unwrap(), 0);
    }

    #[test]
    fn manager_assignment_rechecks_vacancies_and_rolls_back_failed_assignment() {
        let mut conn = database();
        let existing = create_and_assign(&mut conn, 1, "First", ManagerBattingPolicy::LetBatterDecide).unwrap().unwrap();
        assert!(create_and_assign(&mut conn, 1, "Replacement", ManagerBattingPolicy::SeekLineDrive).unwrap().is_none());
        assert_eq!(count(&conn), 1);
        assert_eq!(load_team_manager(&conn, 1).unwrap(), Some(existing));
        assert!(create_and_assign(&mut conn, 999, "Unknown team", ManagerBattingPolicy::Situational).is_err());
        assert!(create_and_assign(&mut conn, 2, " ", ManagerBattingPolicy::Situational).is_err());
        conn.execute_batch("CREATE TRIGGER reject_assignment BEFORE INSERT ON team_manager WHEN NEW.team_id=2 BEGIN SELECT RAISE(ABORT,'test rejection'); END;").unwrap();
        assert!(create_and_assign(&mut conn, 2, "Must roll back", ManagerBattingPolicy::PutBallInPlay).is_err());
        assert_eq!(count(&conn), 1);
        assert!(load_team_manager(&conn, 2).unwrap().is_none());
    }

    #[test]
    fn manager_generation_handles_inclusive_rng_upper_bound() {
        let mut service = ManagerService { repo: MemoryRepo { conn: database() } };
        let mut rng = crate::domain::random_provider::FixedRng::new(1.0);
        assert_eq!(service.generate_missing_managers("jp", &mut rng).unwrap(), 3);
        for team in [1, 2, 3] {
            assert_eq!(load_team_manager(&service.repo.conn, team).unwrap().unwrap().batting_policy, ManagerBattingPolicy::Situational);
        }
    }

    #[test]
    fn manager_generation_reports_missing_name_data_without_creating_orphans() {
        let mut service = ManagerService { repo: MemoryRepo { conn: database() } };
        assert!(service.generate_missing_managers("missing", &mut RealRng::from_seed(7)).is_err());
        assert_eq!(count(&service.repo.conn), 0);
        assert_eq!(service.repo.unmanaged_team_ids().unwrap(), vec![1, 2, 3]);
    }
}
