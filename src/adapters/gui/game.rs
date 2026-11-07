use super::locale::{tr,bilingual};
use dioxus::prelude::*;
use std::sync::Arc;
use crate::application::viewing::{self,Catalog};
use crate::domain::shared::{game::GameDetail,game_cursor::GameCursor};
use super::{blocking,tables::{DataTable,ErrorPanel}};

#[component]
pub(super) fn Games(catalog:Catalog,league:u16,team:u16,refresh:u32)->Element {
    let mut season=use_signal(||catalog.seasons.first().copied().unwrap_or(0));
    let mut selected=use_signal(||None::<u32>);
    let mut query=use_signal(String::new);
    let mut page=use_signal(||0usize);
    let catalog_for_load=catalog.clone();
    let data=use_resource(move || {
        let season=season();let catalog=catalog_for_load.clone();
        // Manual refresh is propagated through the component's key.
        async move {blocking(move ||viewing::games(season,league,team,&catalog)).await}
    });
    if let Some(id)=selected() {return rsx!{GamePanel {key:"game-{id}-{refresh}",id,refresh,onclose:move |_|selected.set(None)}};}
    rsx!{
        div {class:"toolbar",
            label {{tr("シーズン")} select {id:format!("season-filter"),value:season().to_string(),onchange:move |e|{season.set(e.value().parse().unwrap_or(0));page.set(0);},
                if catalog.seasons.is_empty(){option {value:"0",{tr("完了試合なし")}}}
                for year in &catalog.seasons {option {value:year.to_string(),"{year}"}}
            }}
            input {r#type:"search","aria-label":tr("試合を検索"),placeholder:tr("日付・チームを検索…"),value:query,oninput:move |e|{query.set(e.value());page.set(0);}}
        }
        match data.read().as_ref() {
            None=>rsx!{p {class:"loading",{tr("試合を読み込み中…")}}},Some(Err(e))=>rsx!{ErrorPanel {message:e.clone()}},
            Some(Ok(games))=>{
                let q=query().to_lowercase();
                let filtered:Vec<_>=games.iter().filter(|g|format!("{} {} {}",g.actual_date,g.away_team.name,g.home_team.name).to_lowercase().contains(&q)).collect();
                let pages=filtered.len().div_ceil(50).max(1);let current=page().min(pages-1);
                rsx!{
                    p {class:"muted",{bilingual(format!("{} 試合 · 試合を選ぶと詳細を表示します",filtered.len()),format!("{} games · Select a game to view details",filtered.len()))}}
                    if filtered.is_empty(){p {class:"empty",{tr("条件に一致する完了試合はありません。")}}}
                    div {class:"game-list",for game in filtered.iter().skip(current*50).take(50) {
                        button {id:format!("game-card-{}",game.id),class:"game-card",onclick:{let id=game.id;move |_|selected.set(Some(id))},
                            time {"{game.actual_date}"} strong {"{game.away_team.name}"}
                            span {class:"score","{game.away_points} : {game.home_points}"}
                            strong {class:"home","{game.home_team.name}"} span {class:"tag",{tr("詳細 →")}}
                        }
                    }}
                    div {class:"pagination",button {disabled:current==0,onclick:move |_|page.set(current.saturating_sub(1)),{tr("前へ")}}
                        span {"{current+1} / {pages}"} button {disabled:current+1>=pages,onclick:move |_|page.set((current+1).min(pages-1)),{tr("次へ")}}}
                }
            },
        }
    }
}

#[derive(Clone)]
struct SharedGame(Arc<GameDetail>);
impl PartialEq for SharedGame {fn eq(&self,other:&Self)->bool {Arc::ptr_eq(&self.0,&other.0)}}

#[component]
pub(super) fn GamePanel(id:u32,refresh:u32,onclose:EventHandler<()>)->Element {
    let data=use_resource(use_reactive((&id,&refresh),|(id,_refresh)|async move {blocking(move ||viewing::game(id)).await}));
    rsx!{
        div {class:"toolbar",button {onclick:move |_|onclose.call(()),{tr("← 一覧へ")}}}
        match data.read().as_ref() {
            None=>rsx!{p {class:"loading",{tr("試合詳細を読み込み中…")}}},Some(Err(e))=>rsx!{ErrorPanel {message:e.clone()}},
            Some(Ok(game))=>rsx!{GameLoaded {key:"loaded-{id}-{refresh}",game:SharedGame(Arc::new(game.clone()))}},
        }
    }
}

#[component]
fn GameLoaded(game:SharedGame)->Element {
    let mut tab=use_signal(||0u8);
    let cursor=use_signal(||GameCursor::new((*game.0).clone()));
    let final_cursor=use_signal(||viewing::final_game_cursor((*game.0).clone()));
    let final_c=final_cursor.read();
    let detail=&game.0;
    rsx!{
        div {class:"game-title",div {p {class:"eyebrow","{detail.actual_date} / {detail.game_type}"}h2 {"{detail.away_team.name} vs {detail.home_team.name}"}}}
        if tab()!=1 {DataTable {data:viewing::scoreboard(&final_c.final_scoreboard()),interactive:false}}
        div {class:"tabs",
            for (i,label) in [{tr("試合概要")},{tr("一球速報")},{tr("打撃成績")},{tr("投手成績")}].iter().enumerate(){
                button {id:format!("game-tab-{i}"),class:if tab()==i as u8{"active"}else{""},onclick:move |_|tab.set(i as u8),"{label}"}
            }
        }
        match tab(){
            0=>rsx!{
                div {class:"split",
                    section {class:"panel",div {class:"card-body",h3 {{tr("投手結果")}}ul {class:"summary-list",for d in final_c.pitching_decisions(){li {"{d.decision}  {d.pitcher.full_name()}"}}}
                        h3 {{tr("本塁打")}}ul {class:"summary-list",for h in final_c.home_run_summaries(){li {{format!("{}  {} / {}",h.batter.full_name(),crate::I18nManager::global().homerun(h.season_home_runs),crate::I18nManager::global().inning(h.inning_seq,h.inning_tb))}}}}
                    }}
                    section {class:"panel",div {class:"card-body",h3 {{tr("バッテリー")}}for b in final_c.battery_summaries(){
                        p {strong {"{b.team_name}"}br {} {format!("{}: {}",tr("投手"), b.pitchers.iter().map(|p|p.full_name()).collect::<Vec<_>>().join(" → "))}br {}{format!("{}: {}",tr("捕手"), b.catchers.iter().map(|p|p.full_name()).collect::<Vec<_>>().join(" → "))}}
                    }}}
                }
            },
            1=>rsx!{Replay {game:game.clone(),cursor}},
            2=>rsx!{DataTable {data:viewing::game_batting(&final_c,detail.away_team.id,&detail.away_team.name),interactive:false}DataTable {data:viewing::game_batting(&final_c,detail.home_team.id,&detail.home_team.name),interactive:false}},
            _=>rsx!{DataTable {data:viewing::game_pitching(&final_c,detail.away_team.id,&detail.away_team.name),interactive:false}DataTable {data:viewing::game_pitching(&final_c,detail.home_team.id,&detail.home_team.name),interactive:false}},
        }
    }
}

#[component]
fn Replay(game:SharedGame,mut cursor:Signal<GameCursor>)->Element {
    let mut c=cursor.read().clone();
    if !c.has_counts(){return rsx!{p {class:"empty",{tr("この試合の一球記録はありません。")}}};}
    let count=c.current_count();
    let mut positions:Vec<_>=game.0.innings.iter().flat_map(|i|i.counts.iter().map(move |c|(i.seq,i.tb,c.seq))).collect();
    positions.sort_by_key(|(_,_,seq)|*seq);
    let current=positions.iter().position(|(_,_,seq)|*seq==c.count_seq).unwrap_or(0);
    let max=positions.len().saturating_sub(1);
    let current_board = viewing::scoreboard(&c.current_scoreboard());
    let batting=c.current_batting_view();
    let batter=batting.map(|b|b.batter.full_name()).unwrap_or_else(||"—".into());
    let outcome=batting.and_then(|b|b.outcome().ok()).unwrap_or_else(||{tr("記録なし")}.into());
    let pitching=c.current_pitching_view().ok();
    let pitcher=pitching.and_then(|p|game.0.player_entries.iter().find(|e|e.player.id==p.pitcher_id).map(|e|e.player.full_name())).unwrap_or_else(||"—".into());
    let runners=[c.has_runner_on_first(),c.has_runner_on_second(),c.has_runner_on_third()];
    let half=if matches!(c.inning_tb,crate::domain::shared::game::TB::Top){tr("表")}else{tr("裏")};
    rsx!{
        div {class:"replay-controls",
            button {id:format!("replay-prev"),disabled:current==0,onclick:move |_|cursor.write().prev(),{tr("← 前の球")}}
            input {r#type:"range","aria-label":tr("投球を選択"),min:"0",max:max.to_string(),value:current.to_string(),
                oninput:move |e|{if let Ok(index)=e.value().parse::<usize>(){if let Some((inning,tb,seq))=positions.get(index){let mut c=cursor.write();c.inning_seq=*inning;c.inning_tb=*tb;c.count_seq=*seq;}}}}
            button {id:format!("replay-next"),disabled:current==max,onclick:move |_|cursor.write().next(),{tr("次の球 →")}}
        }
        p {class:"muted",{bilingual(format!("{}回{} · 投球 {} / {} · {} 攻撃",c.inning_seq,half,current+1,max+1,c.current_batting_team_name()),format!("{} {} · Pitch {} / {} · {} batting",half,c.inning_seq,current+1,max+1,c.current_batting_team_name()))}}
        DataTable {data:current_board,interactive:false}
        div {class:"split",
            section {class:"panel",div {class:"card-body",
                h3 {"{pitcher} → {batter}"}
                div {class:"metric-grid",
                    div {class:"metric",small {"BALL"}strong {"{count.ball}"}}
                    div {class:"metric",small {"STRIKE"}strong {"{count.strike}"}}
                    div {class:"metric",small {"OUT"}strong {"{count.out}"}}
                }
                h2 {"{outcome}"}
                if let Some(p)=pitching {
                    p {"{p.ball.pitch_type:?} · {p.ball.speed*3.6:.1} km/h"}
                    PitchZone {x:p.ball.actual_location.x,y:p.ball.actual_location.y}
                }
                if let Some(b)=batting {if b.ball.launch_speed>0.0 {p {class:"muted",{bilingual(format!("打球速度 {:.1} km/h / 打球角度 {:.1}°",b.ball.launch_speed*3.6,b.ball.launch_angle),format!("Exit velocity {:.1} km/h / Launch angle {:.1}°",b.ball.launch_speed*3.6,b.ball.launch_angle))}}}}
            }}
            section {class:"panel",div {class:"card-body",h3 {{tr("走者状況")}}
                svg {class:"field",view_box:"0 0 300 260",role:"img","aria-label":tr("塁上の走者"),
                    path {d:"M150 230 L30 110 Q150 -70 270 110 Z",fill:"#24735d"}
                    path {d:"M150 220 L65 135 L150 50 L235 135 Z",fill:"#b69b71",stroke:"#d3dec8",stroke_width:"2"}
                    path {d:"M150 195 L90 135 L150 75 L210 135 Z",fill:"#2f7053"}
                    for (i,(x,y)) in [(230,130),(145,45),(60,130)].into_iter().enumerate(){
                        rect {x:"{x}",y:"{y}",width:"12",height:"12",transform:"rotate(45 {x} {y})",fill:if runners[i]{"#ffe36c"}else{"white"}}
                    }
                    text {x:"150",y:"248",fill:"white",text_anchor:"middle","HOME"}
                }
                if let Some(r)=c.current_running_view(){
                    p {{format!("{}: {}",tr("一塁"),r.runner_1st.as_ref().map(|p|p.full_name()).unwrap_or_else(||"—".into()))}}
                    p {{format!("{}: {}",tr("二塁"),r.runner_2nd.as_ref().map(|p|p.full_name()).unwrap_or_else(||"—".into()))}}
                    p {{format!("{}: {}",tr("三塁"),r.runner_3rd.as_ref().map(|p|p.full_name()).unwrap_or_else(||"—".into()))}}
                }
            }}
        }
    }
}

#[component]
fn PitchZone(x:f64,y:f64)->Element {
    let extent=x.abs().max(y.abs()).max(2.0)+0.4;
    rsx!{
        svg {class:"pitch-zone",view_box:"{-extent} {-extent} {extent*2.0} {extent*2.0}",role:"img","aria-label":tr("投球コース（捕手視点）"),
            rect {x:"-1",y:"-1",width:"2",height:"2",fill:"#e0efe5",stroke:"#406a5d",stroke_width:"0.025"}
            for offset in [-1.0/3.0,1.0/3.0]{
                line {x1:"{offset}",y1:"-1",x2:"{offset}",y2:"1",stroke:"#a9c8ba",stroke_width:"0.015"}
                line {x1:"-1",y1:"{offset}",x2:"1",y2:"{offset}",stroke:"#a9c8ba",stroke_width:"0.015"}
            }
            circle {cx:"{x}",cy:"{-y}",r:"0.11",fill:"#d6553e",stroke:"white",stroke_width:"0.03"}
        }
        p {class:"muted",{bilingual(format!("捕手視点 / コース X {x:.2}, Y {y:.2}"),format!("Catcher view / Location X {x:.2}, Y {y:.2}"))}}
    }
}
