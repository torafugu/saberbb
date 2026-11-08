//! Read-only projections. No simulation, writes, or contact-state diagnostics here.
use crate::{app_context, repositories::{game_repository::{GameDetailReader, GamePlayerReader, GameScheduleReader, ProcessedGameReader},
    schedule_repository::ScheduleRepository, statistics_repository::StatRepository, db::SqlDb}};
use crate::domain::shared::{game::{GameDetail, GameHeader, TeamGameScheduleView}, game_cursor::{GameCursor, ScoreBoard}, player::Player};
use anyhow::Result;
use chrono::NaiveDate;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Choice { pub id: u16, pub name: String, pub league: u16 }
#[derive(Clone, Debug, PartialEq)]
pub struct Catalog {
    pub leagues: Vec<Choice>, pub teams: Vec<Choice>, pub seasons: Vec<u16>,
    pub player_teams: HashMap<i64, u16>,
}
impl Catalog {
    pub fn includes_team(&self, id: u16, league: u16, team: u16) -> bool {
        (team == 0 || team == id) && (league == 0 || self.teams.iter().any(|t| t.id == id && t.league == league))
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct TableData { pub title: String, pub columns: Vec<String>, pub rows: Vec<Vec<String>> }
impl TableData {
    pub fn new(title: impl Into<String>, columns: &[&str], rows: Vec<Vec<String>>) -> Self {
        Self { title: title.into(), columns: columns.iter().map(|s| (*s).into()).collect(), rows }
    }
}

pub fn catalog() -> Result<Catalog> {
    let leagues = app_context().schedule_repository.load_all_leagues()?;
    let seasons = app_context().game_repository.load_processed_seasons()?;
    let conn = SqlDb::new()?.get_conn()?;
    let mut query = conn.prepare("SELECT id,team_id FROM player_info")?;
    let player_teams = query.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<HashMap<i64,u16>>>()?;
    Ok(Catalog {
        teams: leagues.iter().flat_map(|l| l.teams.iter().map(move |t| Choice { id:t.id, name:t.name.to_string(), league:l.id })).collect(),
        leagues: leagues.iter().map(|l| Choice { id:l.id, name:l.name.to_string(), league:l.id }).collect(),
        seasons, player_teams,
    })
}

pub fn games(season: u16, league: u16, team: u16, catalog: &Catalog) -> Result<Vec<GameHeader>> {
    if season == 0 { return Ok(Vec::new()); }
    Ok(app_context().game_repository.load_processed_game_headers(season)?.into_iter()
        .filter(|g| catalog.includes_team(g.away_team.id, league, team) || catalog.includes_team(g.home_team.id, league, team)).collect())
}
pub fn game(id: u32) -> Result<GameDetail> { Ok(app_context().game_repository.load_game_detail(id)?) }
pub fn players(team: u16) -> Result<Vec<Player>> { Ok(app_context().game_repository.load_team_players(team)?) }
pub fn schedule(team: u16, from: NaiveDate, to: NaiveDate) -> Result<Vec<TeamGameScheduleView>> {
    Ok(app_context().game_repository.load_team_game_schedules(team, from, to)?)
}
pub fn manager(team: u16) -> Result<Option<crate::domain::shared::batting_personnel::Manager>> {
    let conn = SqlDb::new()?.get_conn()?;
    crate::repositories::batting_personnel_repository::load_team_manager(&conn, team)
}

/// Statistics retain the existing repository's all-saved-games aggregation scope.
pub fn statistics(kind: u8, league: u16, team: u16, catalog: &Catalog) -> Result<TableData> {
    let repo = &app_context().statistics_repository;
    let includes_player = |id: i64| catalog.player_teams.get(&id).is_some_and(|id| catalog.includes_team(*id, league, team));
    match kind {
        1 => {
            let standings = repo.load_standings()?;
            Ok(standings_table(standings,league,team,catalog))
        }

        2 => {
            let stats = if team == 0 { repo.load_batting_stats()? } else { repo.load_team_batting_stats(team)? };
            let rows = stats.into_iter().filter(|s| includes_player(s.batter.info.id)).map(|s|
                vec![s.batter.info.full_name(),s.ab.to_string(),s.single.to_string(),s.double.to_string(),s.triple.to_string(),s.homerun.to_string(),format!("{:.3}",s.ba),format!("{:.0}",s.rbi)]).collect();
            Ok(TableData::new("打撃成績", &["選手","AB","1B","2B","3B","HR","AVG","RBI"], rows))
        }
        _ => {
            let stats = if team == 0 { repo.load_pitching_stats()? } else { repo.load_team_pitching_stats(team)? };
            let rows = stats.into_iter().filter(|s| includes_player(s.batter.info.id)).map(|s|
                vec![s.batter.info.full_name(),s.games.to_string(),s.innings.to_string(),s.wins.to_string(),s.losses.to_string(),s.saves.to_string(),s.holds.to_string(),format!("{:.2}",s.era),s.so.to_string(),s.bb.to_string()]).collect();
            Ok(TableData::new("投手成績", &["選手","登板","回","勝","敗","S","H","ERA","SO","BB"], rows))
        }
    }
}

fn decimal_rate(value:f64)->String {
    let value=format!("{value:.3}");
    value.strip_prefix('0').unwrap_or(&value).to_string()
}
fn standings_table(mut standings:Vec<crate::domain::shared::stats::Standing>,league:u16,team:u16,catalog:&Catalog)->TableData {
    standings.sort_by(|a,b|b.pct.total_cmp(&a.pct).then_with(||b.wins.cmp(&a.wins)).then_with(||a.team.id.cmp(&b.team.id)));
    let mut leaders=HashMap::new();
    let mut ranks=HashMap::new();
    let mut rows=Vec::new();
    for s in standings {
        let team_league=catalog.teams.iter().find(|t|t.id==s.team.id).map(|t|t.league).unwrap_or(0);
        let (w,l)=*leaders.entry(team_league).or_insert((s.wins as f32,s.losses as f32));
        let state=ranks.entry(team_league).or_insert((0u32,-1.0f32,0u16,0u32));
        state.0+=1;
        if state.1!=s.pct || state.2!=s.wins {state.3=state.0;}
        state.1=s.pct;state.2=s.wins;
        let rank=state.3;
        if !catalog.includes_team(s.team.id,league,team){continue;}
        let gb=((w-s.wins as f32)+(s.losses as f32-l))/2.0;
        rows.push(vec![rank.to_string(),s.team.name.to_string(),s.games.to_string(),s.wins.to_string(),s.losses.to_string(),s.draws.to_string(),
            decimal_rate(s.pct as f64),format!("{gb:.1}"),s.remaining.to_string(),s.r.to_string(),s.ra.to_string(),s.homeruns.to_string(),s.steals.to_string(),
            s.ba.map(decimal_rate).unwrap_or_else(||"—".into()),s.era.map(|v|format!("{v:.2}")).unwrap_or_else(||"—".into()),s.errors.to_string()]);
    }
    TableData::new("順位表", &["順位","チーム","試合","勝","敗","分","勝率","ゲーム差","残試合","得点","失点","本塁打","盗塁","打率","防御率","失策"],rows)
}

pub fn scoreboard(board: &ScoreBoard) -> TableData {
    let mut columns = vec!["チーム".to_owned()];
    columns.extend((1..=board.max_inning_num).map(|n| n.to_string())); columns.push("R".into());
    let row = |name: &str, points: &[u8], total: u8, home: bool| {
        let mut cells = vec![name.into()];
        cells.extend((0..board.max_inning_num as usize).map(|i| {
            if home && board.is_last_bottom_inning_skiped && i+1 == board.max_inning_num as usize { "x".into() }
            else { points.get(i).map(u8::to_string).unwrap_or_else(|| "—".into()) }
        }));
        cells.push(total.to_string()); cells
    };
    TableData { title:"スコアボード".into(), columns, rows:vec![row(&board.away_team_name,&board.away_innning_points,board.away_total_point,false),row(&board.home_team_name,&board.home_innning_points,board.home_total_point,true)] }
}

/// GameCursor::is_last_count is relative to one half-inning, not the whole game.
pub fn final_game_cursor(game: GameDetail) -> GameCursor {
    let last = game.innings.iter().flat_map(|inning| inning.counts.iter()
        .map(move |count| (inning.seq, inning.tb, count.seq))).max_by_key(|(_,_,seq)| *seq);
    let mut cursor = GameCursor::new(game);
    if let Some((inning, tb, count)) = last {
        cursor.inning_seq = inning; cursor.inning_tb = tb; cursor.count_seq = count;
    }
    cursor
}

pub fn game_batting(cursor: &GameCursor, team: u16, title: &str) -> TableData {
    TableData::new(title, &["打順","位置","選手","PA","AB","H","2B","3B","HR"],
        cursor.current_batting_stats_for_team(team).into_iter().map(|s| vec![s.batting_order.to_string(),s.position.to_string(),s.player.full_name(),s.plate_appearances.to_string(),s.at_bats.to_string(),s.hits.to_string(),s.doubles.to_string(),s.triples.to_string(),s.home_runs.to_string()]).collect())
}
pub fn game_pitching(cursor: &GameCursor, team: u16, title: &str) -> TableData {
    TableData::new(title, &["選手","球数","AB","H","R","回","SO","BB","ERA","WHIP"],
        cursor.current_pitching_stats_for_team(team).into_iter().map(|s| vec![s.player.full_name(),s.pitch_count.to_string(),s.at_bats.to_string(),s.hits.to_string(),s.runs.to_string(),s.innings.to_string(),s.strikeouts.to_string(),s.walks.to_string(),format!("{:.2}",s.era),format!("{:.2}",s.whip)]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_filters_teams_without_leaking_other_leagues() {
        let c = Catalog { leagues:vec![], teams:vec![Choice {id:1,name:"A".into(),league:7},Choice {id:2,name:"B".into(),league:8}],seasons:vec![],player_teams:HashMap::new() };
        assert!(c.includes_team(1,7,0)); assert!(!c.includes_team(2,7,0));
        assert!(!c.includes_team(1,0,2)); assert!(c.includes_team(2,0,2));
    }
    #[test]
    fn final_box_score_cursor_includes_later_half_innings() {
        use crate::domain::shared::game::{Count,Inning,TB,GameType};
        use crate::domain::shared::team::Team;
        let count = |seq| Count {seq,point:0,ball:0,strike:0,out:3};
        let game = GameDetail {id:1,actual_date:NaiveDate::from_ymd_opt(2026,4,1).unwrap(),away_team:Team::min(1,"A"),home_team:Team::min(2,"B"),game_type:GameType::Regular,
            innings:vec![Inning {seq:1,tb:TB::Top,counts:vec![count(1),count(2)]},Inning {seq:1,tb:TB::Bottom,counts:vec![count(3)]},Inning {seq:2,tb:TB::Top,counts:vec![count(9)]}],
            away_points:0,home_points:0,player_entries:vec![],player_pitchings:vec![],player_pitching_decisions:vec![],player_home_runs:vec![],player_battings:vec![],player_runnings:vec![]};
        let cursor=final_game_cursor(game);
        assert_eq!((cursor.inning_seq,cursor.inning_tb,cursor.count_seq),(2,TB::Top,9));
    }

    #[test]
    fn scoreboard_keeps_missing_innings_distinct_from_zero_and_marks_unplayed_bottom() {
        let board = ScoreBoard {away_team_name:"A".into(),home_team_name:"B".into(),max_inning_num:2,away_total_point:0,home_total_point:1,away_innning_points:vec![0,0],home_innning_points:vec![1],is_last_bottom_inning_skiped:true};
        let table = scoreboard(&board);
        assert_eq!(table.rows[0],vec!["A","0","0","0"]);
        assert_eq!(table.rows[1],vec!["B","1","x","1"]);
    }
    #[test]
    fn rate_display_omits_only_leading_zero_and_preserves_one() {
        assert_eq!(decimal_rate(0.0),".000");assert_eq!(decimal_rate(0.125),".125");
        assert_eq!(decimal_rate(2.0/3.0),".667");assert_eq!(decimal_rate(1.0),"1.000");
    }
    #[test]
    fn standings_keep_rank_and_gap_when_filtering_a_team() {
        use crate::domain::shared::{stats::Standing,team::Team};
        let catalog=Catalog {leagues:vec![],teams:vec![Choice{id:1,name:"A".into(),league:1},Choice{id:2,name:"B".into(),league:1}],seasons:vec![],player_teams:HashMap::new()};
        let standing=|id,wins,losses| Standing {team:Team::min(id,"Team"),games:wins+losses,wins,losses,draws:0,pct:wins as f32/(wins+losses) as f32,gb:0.0,
            r:10,ra:5,remaining:3,homeruns:1,steals:2,ba:Some(0.25),era:Some(2.5),errors:1};
        let table=standings_table(vec![standing(1,8,2),standing(2,6,4)],1,2,&catalog);
        assert_eq!(table.columns.len(),16);assert_eq!(table.rows.len(),1);
        assert_eq!(table.rows[0][0],"2");assert_eq!(table.rows[0][6],".600");assert_eq!(table.rows[0][7],"2.0");
        assert_eq!(table.rows[0][13],".250");assert_eq!(table.rows[0][14],"2.50");
    }

}
