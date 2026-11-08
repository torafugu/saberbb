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
        "集計範囲：保存済み全試合（通算）"=>"Scope: all saved games (career)", "同じローカルDBから表示"=>"Using the same local database",
        "データベースを読み込み中…"=>"Loading database…", "成績を読み込み中…（初回のみ集計します）"=>"Loading statistics… (one-time backfill on first use)",
        "別の処理がDBを使用中です。集計・試合保存の完了を待ち、DB編集ツールのトランザクションを終了して再読み込みしてください。"=>"The database is in use. Wait for statistics or game saves to finish, end transactions in database editing tools, then refresh.",
        "読み込みできませんでした"=>"Unable to load data", "DB設定と初期化を確認し、再読み込みしてください。"=>"Check your database settings and setup, then refresh.",
        "選手・チームを検索…"=>"Search players or teams…", "表を検索"=>"Search table", "前へ"=>"Previous", "次へ"=>"Next",

        "シーズン"=>"Season", "全シーズン"=>"All seasons", "集計範囲：選択シーズン"=>"Scope: selected season", "完了試合なし"=>"No completed games", "日付・チームを検索…"=>"Search date or team…", "試合を検索"=>"Search games",
        "試合を読み込み中…"=>"Loading games…", "条件に一致する完了試合はありません。"=>"No completed games match the filters.",
        "← 一覧へ"=>"← Back to list", "試合詳細を読み込み中…"=>"Loading game details…", "スコアボード"=>"Scoreboard", "試合概要"=>"Summary", "一球速報"=>"Pitch by pitch",
        "投手結果"=>"Pitching decisions", "本塁打"=>"Home runs", "バッテリー"=>"Batteries", "この試合の一球記録はありません。"=>"No pitch records for this game.",
        "← 前の球"=>"← Previous pitch", "次の球 →"=>"Next pitch →", "投球を選択"=>"Select pitch", "記録なし"=>"No record", "走者状況"=>"Baserunners",
        "塁上の走者"=>"Runners on base", "投球コース（捕手視点）"=>"Pitch location (catcher view)",
        "上のフィルターからチームを選択してください。選手の能力・球種とチーム日程を表示します。"=>"Select a team above to view player abilities, pitch repertoires and the team schedule.",
        "選手・監督"=>"Players & manager", "チーム日程"=>"Team schedule", "選手を読み込み中…"=>"Loading players…",

        "ポジション"=>"Position", "全選手"=>"All players", "投手"=>"Pitcher", "捕手"=>"Catcher", "内野・DH"=>"Infield & DH", "外野"=>"Outfield",
        "選手"=>"Player", "背番号"=>"Number", "年齢"=>"Age", "守備位置"=>"Position", "打撃傾向"=>"Batting character",
        "打撃能力"=>"Batting abilities", "走塁能力"=>"Running abilities", "守備・投球能力"=>"Defense & pitching", "パラメータ"=>"Parameter", "値"=>"Value",
        "月"=>"Month", "日程を読み込み中…"=>"Loading schedule…", "この月の日程はありません。"=>"No games scheduled for this month.", "結果を見る"=>"View result", "未実施"=>"Scheduled",
        "試合"=>"G", "勝"=>"W", "敗"=>"L", "分"=>"T", "勝率"=>"PCT", "ゲーム差"=>"GB", "登板"=>"G", "回"=>"IP", "打順"=>"Order", "位置"=>"Pos", "球数"=>"Pitches",
        "管理"=>"Management",
        "閲覧も管理もこの画面から"=>"Browse and manage here",
        "管理処理を実行中… 進捗を見る"=>"Management task running… View progress",
        "スケジュール作成"=>"Create schedule",
        "選手・監督作成"=>"Generate players & managers",
        "試合進行"=>"Process games",
        "分析用の試合進行"=>"Process with diagnostics",
        "指定年数分のシーズン日程を追加します。"=>"Add season schedules for the requested number of years.",
        "選手を作成してチームに分配し、監督がいないチームに監督を配置します。0人で監督のみ補充できます。"=>"Generate and distribute players, then assign managers to vacant teams. Use 0 to fill manager vacancies only.",
        "スケジュール済みの試合を進めます。詳細な接触診断データは記録しません。"=>"Process scheduled game rounds without recording detailed contact diagnostics.",
        "試合を進め、詳細な接触診断データを記録します。データ容量が増えます。"=>"Process games and record detailed contact diagnostics. This increases database size.",
        "シーズン数"=>"Seasons",
        "選手数"=>"Players",
        "試合節数"=>"Rounds",
        "実行"=>"Run",
        "処理の進捗"=>"Task progress",
        "実行中…"=>"Running…",
        "処理に失敗しました"=>"Task failed",
        "完了しました"=>"Completed",
        "リーグ全体が対象です。各処理の完了後に試合結果・成績・選手一覧を自動更新します。"=>"Operations apply across the leagues. Results, statistics and rosters refresh automatically after each task.",
        "実行中はアプリを閉じないでください。別の画面を閲覧しても処理は継続します。"=>"Keep the app open while running. Tasks continue when you browse another page.",
        "途中まで保存された結果は保持されます。進捗を確認してから再実行してください。"=>"Previously saved work is retained. Check progress before retrying.",
        "表示できるデータがありません。管理画面で作成・進行できます。"=>"No data available. Create characters, schedule or process games from Management.",
        "監督未配置（管理画面の選手・監督作成で補充できます）"=>"No manager assigned. Generate players & managers in Management to fill vacancies.",
        "順位"=>"Rank", "残試合"=>"Remaining", "得点"=>"Runs", "失点"=>"Runs allowed", "盗塁"=>"SB", "打率"=>"AVG", "防御率"=>"ERA", "失策"=>"Errors",
        "防御率は失点と記録アウト数による参考値です（自責点は未記録）。"=>"ERA is a reference value based on runs allowed and recorded outs; earned runs are not recorded.",
        _=>source,
    };
    translated.into()
}

#[component]
pub fn LanguageSwitch(#[props(default)] disabled:bool)->Element {
    let mut selected=use_context::<Signal<Language>>();
    let mut error=use_signal(||None::<String>);
    rsx!{div {class:"language-controls",
        for (choice,label) in [(Language::Japanese,"日本語"),(Language::English,"English")] {
            button {disabled,id:format!("language-{}",choice.code()),aria_pressed: selected()==choice,
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
