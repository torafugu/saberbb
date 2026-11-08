//! Headless integration checks dispatch real Dioxus DOM events, then snapshot the renderer.
use dioxus::prelude::*;
use dioxus::dioxus_core::{Mutation,Mutations,AttributeValue,ElementId};
use dioxus::html::{EventData,SerializedMouseData,SerializedFormData,SerializedHtmlEventConverter};
use std::{collections::HashMap,path::Path};
use crate::application::viewing;

struct Harness { dom:VirtualDom, ids:HashMap<String,ElementId> }
impl Harness {
    fn apply(&mut self, mutations:Mutations) {
        for edit in mutations.edits {
            if let Mutation::SetAttribute {name:"id",value:AttributeValue::Text(name),id,..}=edit {self.ids.insert(name,id);}
        }
    }
    async fn settle(&mut self) {
        let edits=self.dom.render_immediate_to_vec();self.apply(edits);
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(120);
        loop {
            assert!(std::time::Instant::now()<deadline,"GUI did not settle within 120 seconds");
            if tokio::time::timeout(std::time::Duration::from_millis(100),self.dom.wait_for_work()).await.is_err(){
                if self.html().contains("class=\"loading\"") || self.html().contains("class=\"job-banner\"") {continue;} else {break;}
            }
            let edits=self.dom.render_immediate_to_vec();self.apply(edits);
        }
    }
    fn event(&self,id:&str,name:&str,data:EventData)->anyhow::Result<()> {
        let id=*self.ids.get(id).ok_or_else(||anyhow::anyhow!("missing GUI control: {id}"))?;
        self.dom.runtime().handle_event(name,Event::new(data.into_any(),true),id);Ok(())
    }
    async fn click(&mut self,id:&str)->anyhow::Result<()> {
        self.event(id,"click",EventData::Mouse(SerializedMouseData::default()))?;self.settle().await;Ok(())
    }
    async fn input(&mut self,id:&str,event:&str,value:&str)->anyhow::Result<()> {
        self.event(id,event,EventData::Form(SerializedFormData::new(value.into(),vec![])))?;self.settle().await;Ok(())
    }
    fn html(&self)->String {dioxus_ssr::render(&self.dom)}
    fn save(&self,output:&Path,name:&str)->anyhow::Result<()> {
        std::fs::write(output.join(format!("{name}.html")),format!("<!doctype html><html lang=\"ja\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><body>{}</body></html>",self.html()))?;Ok(())
    }
}

pub async fn snapshot_database(output:&Path)->anyhow::Result<()> {
    std::fs::create_dir_all(output)?;
    dioxus::html::set_event_converter(Box::new(SerializedHtmlEventConverter));
    let catalog=viewing::catalog()?;
    let team=catalog.teams.first().ok_or_else(||anyhow::anyhow!("test DB needs teams"))?.id;
    let mut h=Harness {dom:VirtualDom::new(super::App),ids:HashMap::new()};
    let edits=h.dom.rebuild_to_vec();h.apply(edits);h.settle().await;
    let first=h.ids.keys().filter(|k|k.starts_with("game-card-")).min().cloned().ok_or_else(||anyhow::anyhow!("test DB needs processed games: {}",h.html()))?;
    h.save(output,"games")?;
    h.click(&first).await?;
    anyhow::ensure!(h.html().contains("試合概要"),"game selection did not open detail");
    h.click("game-tab-1").await?;
    anyhow::ensure!(h.html().contains("投球 1 /"),"replay did not start at first pitch");
    h.click("replay-next").await?;
    anyhow::ensure!(h.html().contains("投球 2 /"),"next pitch did not advance");
    h.click("replay-prev").await?;
    anyhow::ensure!(h.html().contains("投球 1 /"),"previous pitch did not return");
    h.save(output,"replay")?;
    h.click("game-tab-2").await?;anyhow::ensure!(h.html().contains(">PA<"),"game batting table missing");
    h.click("game-tab-3").await?;anyhow::ensure!(h.html().contains("WHIP"),"game pitching table missing");
    h.click("nav-2").await?;
    h.save(output,"batting")?;anyhow::ensure!(h.html().contains("RBI"),"batting statistics failed; inspect batting.html");
    h.input("table-search","input","__no_such_player__").await?;
    anyhow::ensure!(h.html().contains("表示できるデータがありません"),"table search did not filter rows");
    h.input("table-search","input","").await?;
    h.click("sort-5").await?;
    anyhow::ensure!(h.html().contains("aria-sort=\"ascending\""),"ascending sort did not update");
    h.click("sort-5").await?;
    anyhow::ensure!(h.html().contains("aria-sort=\"descending\""),"descending sort did not update");
    h.save(output,"batting")?;
    h.click("nav-1").await?;anyhow::ensure!(h.html().contains("ゲーム差"),"standings failed");
    for label in ["残試合","得点","失点","本塁打","盗塁","打率","防御率","失策"] {anyhow::ensure!(h.html().contains(label),"missing standings column: {label}");}
    h.save(output,"standings")?;
    h.click("language-en-US").await?;
    anyhow::ensure!(h.html().contains("Remaining") && h.html().contains("Runs allowed"),"standings English labels missing");
    h.save(output,"standings-english")?;
    h.click("language-ja-JP").await?;
    h.click("nav-3").await?;anyhow::ensure!(h.html().contains("ERA"),"pitching statistics failed");
    h.input("team-filter","change",&team.to_string()).await?;
    h.click("nav-4").await?;
    let player=h.ids.keys().filter(|k|k.starts_with("roster-")).min().cloned().ok_or_else(||anyhow::anyhow!("team roster missing"))?;
    h.click(&player).await?;
    anyhow::ensure!(h.html().contains("走塁能力"),"player selection did not show abilities");h.save(output,"players")?;
    h.click("team-tab-1").await?;
    h.input("schedule-month","change","2026-04").await?;
    anyhow::ensure!(h.html().contains("game-card"),"team schedule missing");h.save(output,"schedule")?;
    h.click("refresh").await?;
    anyhow::ensure!(!h.html().contains("読み込みできませんでした"),"refresh failed");
    // Name rows must be reformatted as well as translating chrome and headings.
    h.click("nav-2").await?;
    use crate::repositories::statistics_repository::StatRepository;
    let names=crate::app_context().statistics_repository.load_team_batting_stats(team)?;
    let player=&names.first().ok_or_else(||anyhow::anyhow!("test DB needs team batting stats"))?.batter.info;
    let ja=format!("{} {}",player.last_name,player.first_name);
    let en=format!("{} {}",player.first_name,player.last_name);
    anyhow::ensure!(h.html().contains(&ja),"Japanese names must be family-first");
    h.click("language-en-US").await?;
    anyhow::ensure!(h.html().contains("<h1>Batting</h1>"),"English heading did not update");
    anyhow::ensure!(h.html().contains(&en),"English names must be given-first");
    anyhow::ensure!(crate::config::load_app_config()?.language=="en-US","English preference was not persisted");
    h.save(output,"batting-english")?;
    h.click("language-ja-JP").await?;
    anyhow::ensure!(h.html().contains(&ja),"switching back must restore family-first names");
    anyhow::ensure!(crate::config::load_app_config()?.language=="ja-JP","Japanese preference was not persisted");
    std::fs::write(output.join("index.html"),std::fs::read(output.join("games.html"))?)?;
    println!("GUI event checks passed: game selection, next/previous pitch, box scores, standings, batting, pitching, search, team filter, roster, player details, monthly schedule.");
    Ok(())
}

/// Mutating GUI checks. Run only against an isolated, freshly initialized test DB.
pub async fn snapshot_management(output: &Path) -> anyhow::Result<()> {
    use crate::repositories::db::DbClient;
    std::fs::create_dir_all(output)?;
    dioxus::html::set_event_converter(Box::new(SerializedHtmlEventConverter));
    let db = DbClient::new()?;
    let count = |table: &str| -> anyhow::Result<i64> {
        Ok(db.get_conn()?.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r|r.get(0))?)
    };
    anyhow::ensure!(count("player_info")? == 0 && count("game")? == 0, "management harness requires a fresh test database");
    let mut h=Harness {dom:VirtualDom::new(super::App),ids:HashMap::new()};
    let edits=h.dom.rebuild_to_vec();h.apply(edits);h.settle().await;
    h.click("nav-5").await?;
    h.save(output,"management")?;
    h.click("language-en-US").await?;
    h.save(output,"management-english")?;
    anyhow::ensure!(h.html().contains("Generate players"),"management English translation missing");
    h.click("language-ja-JP").await?;
    h.input("manage-schedule-amount","input","0").await?;
    anyhow::ensure!(h.html().contains("1〜255の整数"),"invalid season count was not rejected");
    h.input("manage-schedule-amount","input","1").await?;
    // Error recovery before any schedules exist.
    h.click("manage-process-run").await?;
    anyhow::ensure!(h.html().contains("処理に失敗しました"),"unscheduled processing should show a recoverable error");
    h.save(output,"management-error")?;
    h.input("manage-generate-amount","input","0").await?;
    h.click("manage-generate-run").await?;
    anyhow::ensure!(count("player_info")? == 0 && count("team_manager")? > 0,"generate 0 did not fill only manager vacancies");
    let managers=count("manager")?;
    h.input("manage-generate-amount","input","600").await?;
    h.click("manage-generate-run").await?;
    anyhow::ensure!(count("player_info")? == 600 && count("manager")? == managers,"generation duplicated managers or lost players");
    h.click("manage-schedule-run").await?;
    anyhow::ensure!(count("game")? > 0,"GUI scheduling did not save games");
    // Dispatch a second click and navigate before the worker completes. The root
    // controller must reject duplicates and keep working after the form unmounts.
    h.event("manage-process-run","click",EventData::Mouse(SerializedMouseData::default()))?;
    h.event("manage-process-run","click",EventData::Mouse(SerializedMouseData::default()))?;
    let edits=h.dom.render_immediate_to_vec();h.apply(edits);
    anyhow::ensure!(h.html().contains("class=\"job-banner\""),"busy status missing");
    h.save(output,"management-running")?;
    h.event("nav-0","click",EventData::Mouse(SerializedMouseData::default()))?;
    h.settle().await;
    h.click("nav-5").await?;
    h.save(output,"management-process-result")?;
    h.click("nav-0").await?;
    let round:i64=db.get_conn()?.query_row("SELECT current_round_seq FROM game_season",[],|r|r.get(0))?;
    anyhow::ensure!(round==2,"expected exactly one completed round, current_round_seq={round}; inspect management-process-result.html");
    anyhow::ensure!(count("player_game_batting")? > 0 && count("player_game_contact_state")? == 0,"normal GUI processing recorded contact diagnostics or saved no games");
    anyhow::ensure!(h.html().contains("game-card"),"completed games did not appear after automatic refresh");
    h.click("nav-5").await?;
    anyhow::ensure!(h.html().contains("完了しました"),"task result did not survive navigation");
    h.click("manage-analyze-run").await?;
    anyhow::ensure!(count("player_game_contact_state")? > 0,"analysis processing did not record diagnostics");
    h.save(output,"management-complete")?;
    h.click("nav-2").await?;
    anyhow::ensure!(h.html().contains("RBI") && !h.html().contains("読み込みできませんでした"),"batting stats not available after GUI processing");
    h.click("nav-3").await?;
    anyhow::ensure!(h.html().contains("ERA") && !h.html().contains("読み込みできませんでした"),"pitching stats not available after GUI processing");
    println!("GUI management checks passed: validation, languages, error recovery, manager-only and player generation, scheduling, processing, diagnostics, duplicate suppression, navigation survival, automatic refresh and statistics.");
    Ok(())
}
