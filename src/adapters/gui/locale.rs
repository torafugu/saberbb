use dioxus::prelude::*;
use crate::I18nManager;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Language { Japanese, English }
impl Language {
    pub fn code(self)->&'static str {if self==Self::Japanese {"ja-JP"}else{"en-US"}}
    pub fn current()->Self {if I18nManager::global().is_japanese(){Self::Japanese}else{Self::English}}
}
pub fn language()->Language {
    try_consume_context::<Signal<Language>>().map(|s|*s.read()).unwrap_or_else(Language::current)
}
pub fn bilingual(ja:String,en:String)->String {if language()==Language::Japanese{ja}else{en}}
pub fn tr(source:&str)->String { translate(source,language()) }
pub fn translate(source:&str,language:Language)->String {
    if language==Language::Japanese{return source.into();}
    let translated=match source {
        "表"=>"Top", "裏"=>"Bottom", "件"=>"rows", "一塁"=>"First base", "二塁"=>"Second base", "三塁"=>"Third base", "監督"=>"Manager",
        "試合結果"=>"Games", "順位表"=>"Standings", "打撃成績"=>"Batting", "投手成績"=>"Pitching", "チーム・選手"=>"Teams & players",
        "再読み込み"=>"Refresh", "↻ 再読み込み"=>"↻ Refresh", "リーグ"=>"League", "チーム"=>"Team", "全リーグ"=>"All leagues", "全チーム"=>"All teams",
        "管理操作はTUIへ"=>"Manage in the TUI", "集計範囲：保存済み全試合（通算）"=>"Scope: all saved games (career)", "同じローカルDBから表示"=>"Using the same local database",
        "データベースを読み込み中…"=>"Loading database…", "成績を読み込み中…（初回のみ集計します）"=>"Loading statistics… (one-time backfill on first use)",
        "別の処理がDBを使用中です。集計・試合保存の完了を待ち、DB編集ツールのトランザクションを終了して再読み込みしてください。"=>"The database is in use. Wait for statistics or game saves to finish, end transactions in database editing tools, then refresh.",
        "読み込みできませんでした"=>"Unable to load data", "DB設定と初期化を確認し、再読み込みしてください。"=>"Check your database settings and setup, then refresh.",
        "選手・チームを検索…"=>"Search players or teams…", "表を検索"=>"Search table", "前へ"=>"Previous", "次へ"=>"Next",
        "表示できるデータがありません。管理TUIで作成・進行した後、再読み込みしてください。"=>"No data available. Create and process games in the management TUI, then refresh.",
        "シーズン"=>"Season", "完了試合なし"=>"No completed games", "日付・チームを検索…"=>"Search date or team…", "試合を検索"=>"Search games",
        "試合を読み込み中…"=>"Loading games…", "条件に一致する完了試合はありません。"=>"No completed games match the filters.",
        "← 一覧へ"=>"← Back to list", "試合詳細を読み込み中…"=>"Loading game details…", "スコアボード"=>"Scoreboard", "試合概要"=>"Summary", "一球速報"=>"Pitch by pitch",
        "投手結果"=>"Pitching decisions", "本塁打"=>"Home runs", "バッテリー"=>"Batteries", "この試合の一球記録はありません。"=>"No pitch records for this game.",
        "← 前の球"=>"← Previous pitch", "次の球 →"=>"Next pitch →", "投球を選択"=>"Select pitch", "記録なし"=>"No record", "走者状況"=>"Baserunners",
        "塁上の走者"=>"Runners on base", "投球コース（捕手視点）"=>"Pitch location (catcher view)",
        "上のフィルターからチームを選択してください。選手の能力・球種とチーム日程を表示します。"=>"Select a team above to view player abilities, pitch repertoires and the team schedule.",
        "選手・監督"=>"Players & manager", "チーム日程"=>"Team schedule", "選手を読み込み中…"=>"Loading players…",
        "監督未配置（管理TUIのキャラクター作成で補充できます）"=>"No manager assigned. Generate characters in the management TUI to fill vacancies.",
        "ポジション"=>"Position", "全選手"=>"All players", "投手"=>"Pitcher", "捕手"=>"Catcher", "内野・DH"=>"Infield & DH", "外野"=>"Outfield",
        "選手"=>"Player", "背番号"=>"Number", "年齢"=>"Age", "守備位置"=>"Position", "打撃傾向"=>"Batting character",
        "打撃能力"=>"Batting abilities", "走塁能力"=>"Running abilities", "守備・投球能力"=>"Defense & pitching", "パラメータ"=>"Parameter", "値"=>"Value",
        "月"=>"Month", "日程を読み込み中…"=>"Loading schedule…", "この月の日程はありません。"=>"No games scheduled for this month.", "結果を見る"=>"View result", "未実施"=>"Scheduled",
        "試合"=>"G", "勝"=>"W", "敗"=>"L", "分"=>"T", "勝率"=>"PCT", "ゲーム差"=>"GB", "登板"=>"G", "回"=>"IP", "打順"=>"Order", "位置"=>"Pos", "球数"=>"Pitches",
        _=>source,
    };
    translated.into()
}

#[component]
pub fn LanguageSwitch()->Element {
    let mut selected=use_context::<Signal<Language>>();
    let mut error=use_signal(||None::<String>);
    rsx!{div {class:"language-controls",
        for (choice,label) in [(Language::Japanese,"日本語"),(Language::English,"English")] {
            button {id:format!("language-{}",choice.code()),aria_pressed: selected()==choice,
                class:if selected()==choice {"active"}else{""},onclick:move |_| {
                    I18nManager::init(choice.code());selected.set(choice);
                    error.set(crate::config::save_language(choice.code()).err().map(|e|e.to_string()));
                },"{label}"}
        }
        if let Some(message)=error(){span {role:"alert","{message}"}}
    }}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn translations_switch_labels_without_altering_names() {
        assert_eq!(translate("打撃成績",Language::English),"Batting");
        assert_eq!(translate("打撃成績",Language::Japanese),"打撃成績");
        assert_eq!(translate("大谷 翔平",Language::English),"大谷 翔平");
    }
}
