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
        for _ in 0..200 {
            if tokio::time::timeout(std::time::Duration::from_millis(100),self.dom.wait_for_work()).await.is_err(){
                if self.html().contains("class=\"loading\"") {continue;} else {break;}
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
