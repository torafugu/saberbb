use dioxus::prelude::*;
use crate::application::viewing::{self, Catalog};
use super::{blocking, tables::{DataTable, ErrorPanel}, game::Games, teams::Teams};

#[component]
pub fn App() -> Element {
    let mut refresh = use_signal(|| 0u32);
    let catalog = use_resource(move || { refresh(); blocking(viewing::catalog) });
    rsx! {
        style { {include_str!("style.css")} }
        div { class: "app-shell",
            match catalog.read().as_ref() {
                None => rsx! { main { class:"loading", "データベースを読み込み中…" } },
                Some(Err(e)) => rsx! { main { ErrorPanel {message:e.clone()} button {onclick:move |_|refresh+=1,"再読み込み"} } },
                Some(Ok(data)) => rsx! { Browser { catalog:data.clone(), refresh:refresh(), onrefresh:move |_|refresh+=1 } },
            }
        }
    }
}

#[component]
pub(super) fn Browser(catalog: Catalog, refresh:u32, onrefresh: EventHandler<()>) -> Element {
    let mut view = use_signal(|| 0u8);
    let mut league = use_signal(|| 0u16);
    let mut team = use_signal(|| 0u16);
    let headings = ["試合結果", "順位表", "打撃成績", "投手成績", "チーム・選手"];
    let labels = ["Games", "Standings", "Batting", "Pitching", "Teams"];
    rsx! {
        aside { class:"sidebar",
            div { class:"brand", span { class:"brand-mark","S" } div { strong {"SABERBB"} small {"BASEBALL LAB"} } }
            p { class:"nav-label","BROWSE" }
            nav {
                for (i,label) in headings.iter().enumerate() {
                    button { id:format!("nav-{i}"), class:if view()==i as u8 {"nav-item selected"} else {"nav-item"},
                        onclick:move |_|view.set(i as u8), span {"{labels[i]}"} strong {"{label}"} }
                }
            }
            div { class:"sidebar-note", "管理操作はTUIへ" code {"cargo run -- --tui"} }
        }
        main {
            header { class:"topbar", div { p {class:"eyebrow","SEASON ARCHIVE / LIVE DATABASE"} h1 {"{headings[view() as usize]}"} }
                button {id:format!("refresh"),class:"refresh",onclick:move |_|onrefresh.call(()),"↻ 再読み込み"} }
            div { class:"filters",
                label { "リーグ" select { id:format!("league-filter"), value:league().to_string(), onchange:move |e|{league.set(e.value().parse().unwrap_or(0));team.set(0);},
                    option { value:"0","全リーグ" }
                    for l in &catalog.leagues { option {value:l.id.to_string(),"{l.name}"} }
                } }
                label { "チーム" select { id:format!("team-filter"), value:team().to_string(), onchange:move |e|team.set(e.value().parse().unwrap_or(0)),
                    option {value:"0","全チーム"}
                    for t in catalog.teams.iter().filter(|t|league()==0 || t.league==league()) { option {value:t.id.to_string(),"{t.name}"} }
                } }
                span { class:"scope-note", if (1..=3).contains(&view()) {"集計範囲：保存済み全試合（通算）"} else {"同じローカルDBから表示"} }
            }
            match view() {
                0 => rsx!{ Games {key:"games-{league()}-{team()}-{refresh}",catalog:catalog.clone(),league:league(),team:team(),refresh} },
                1..=3 => rsx!{ Statistics {key:"stats-{view()}-{league()}-{team()}",kind:view(),catalog:catalog.clone(),league:league(),team:team(),refresh} },
                _ => rsx!{ Teams {key:"team-{team()}",team:team(),refresh} },
            }
        }
    }
}

#[component]
fn Statistics(kind:u8, catalog:Catalog, league:u16, team:u16, refresh:u32) -> Element {
    let data = use_resource(use_reactive((&kind,&league,&team,&refresh,&catalog), |(kind,league,team,_refresh,catalog)| async move {
        blocking(move || viewing::statistics(kind,league,team,&catalog)).await
    }));
    rsx! { match data.read().as_ref() {
        None=>rsx!{p {class:"loading","成績を集計中…"}},
        Some(Err(e))=>rsx!{ErrorPanel {message:e.clone()}},
        Some(Ok(table))=>rsx!{DataTable {data:table.clone(),initial_sort:if kind==1 {None}else{Some((1,true))}}},
    } }
}
