use super::locale::{bilingual, tr};
use crate::application::management;
use dioxus::prelude::*;
use futures::StreamExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation { Schedule, Generate, Process, Analyze }

impl Operation {
    const ALL: [Self; 4] = [Self::Schedule, Self::Generate, Self::Process, Self::Analyze];
    fn id(self) -> &'static str {
        match self { Self::Schedule => "schedule", Self::Generate => "generate", Self::Process => "process", Self::Analyze => "analyze" }
    }
    fn title(self) -> &'static str {
        match self { Self::Schedule => "スケジュール作成", Self::Generate => "選手・監督作成", Self::Process => "試合進行", Self::Analyze => "分析用の試合進行" }
    }
    fn description(self) -> &'static str {
        match self {
            Self::Schedule => "指定年数分のシーズン日程を追加します。",
            Self::Generate => "選手を作成してチームに分配し、監督がいないチームに監督を配置します。0人で監督のみ補充できます。",
            Self::Process => "スケジュール済みの試合を進めます。詳細な接触診断データは記録しません。",
            Self::Analyze => "試合を進め、詳細な接触診断データを記録します。データ容量が増えます。",
        }
    }
    fn unit(self) -> &'static str {
        match self { Self::Schedule => "シーズン数", Self::Generate => "選手数", _ => "試合節数" }
    }
    fn max(self) -> u16 { if self == Self::Schedule { 255 } else { u16::MAX } }
    fn min(self) -> u16 { if self == Self::Generate { 0 } else { 1 } }
    fn parse(self, value: &str) -> Option<u16> {
        value.parse::<u16>().ok().filter(|n| *n >= self.min() && *n <= self.max())
    }
    fn execute(self, amount: u16, progress: impl FnMut(u16)) -> anyhow::Result<Option<management::GenerationReport>> {
        match self {
            Self::Schedule => management::schedule_seasons_with_progress(amount as u8, progress)?,
            Self::Generate => return management::generate_characters_with_progress(amount, progress).map(Some),
            Self::Process | Self::Analyze => management::process_rounds_with_progress(amount, self == Self::Analyze, progress)?,
        }
        Ok(None)
    }
}

#[derive(Clone, Default)]
pub(super) struct JobState {
    pub busy: bool,
    operation: Option<Operation>,
    amount: u16,
    completed: u16,
    outcome: Option<Result<Option<management::GenerationReport>, String>>,
}

#[derive(Clone, Copy)]
pub(super) struct Controller {
    pub state: Signal<JobState>,
    worker: Coroutine<(Operation, u16)>,
}

impl Controller {
    fn submit(mut self, operation: Operation, amount: u16) {
        if self.state.peek().busy { return; }
        self.state.set(JobState { busy: true, operation: Some(operation), amount, ..Default::default() });
        self.worker.send((operation, amount));
    }
}

// Keep the worker at App scope: navigating away from Management must not cancel a
// job, lose its status, or allow a second writer while spawn_blocking continues.
pub(super) fn provide_controller(mut refresh: Signal<u32>) {
    let mut state = use_signal(JobState::default);
    let worker = use_coroutine(move |mut requests: UnboundedReceiver<(Operation, u16)>| async move {
        while let Some((operation, amount)) = requests.next().await {
            let (progress, mut updates) = tokio::sync::mpsc::unbounded_channel();
            let mut work = tokio::task::spawn_blocking(move || {
                operation.execute(amount, |n| { let _ = progress.send(n); })
            });
            let result = loop {
                tokio::select! {
                    Some(n) = updates.recv() => state.write().completed = n,
                    result = &mut work => break result,
                }
            };
            // Drain messages that raced with the task completing, including the
            // last successfully saved unit before a later failure.
            while let Ok(n) = updates.try_recv() { state.write().completed = n; }
            let outcome = match result {
                Ok(result) => result.map_err(|e| format!("{e:#}")),
                Err(error) => Err(format!("Worker failed: {error}")),
            };
            if outcome.is_ok() { state.write().completed = amount; }
            state.write().outcome = Some(outcome);
            state.write().busy = false;
            // A failed batch may also have saved some work. Always reload.
            refresh += 1;
        }
    });
    use_context_provider(|| Controller { state, worker });
}

#[component]
pub(super) fn Management() -> Element {
    let controller = use_context::<Controller>();
    let job = controller.state.read().clone();
    rsx! {
        p {class:"muted", {tr("リーグ全体が対象です。各処理の完了後に試合結果・成績・選手一覧を自動更新します。")}}
        if let Some(operation) = job.operation {
            section {class:if job.outcome.as_ref().is_some_and(|r|r.is_err()) {"error"} else {"panel job-status"},
                role:if job.outcome.as_ref().is_some_and(|r|r.is_err()) {"alert"} else {"status"}, aria_live:"polite",
                h3 { {tr(operation.title())} " · " {tr(if job.busy {"実行中…"} else if job.outcome.as_ref().is_some_and(|r|r.is_err()) {"処理に失敗しました"} else {"完了しました"})} }
                if job.amount > 0 {
                    p { {bilingual(format!("完了：{} / {}",job.completed,job.amount),format!("Completed: {} / {}",job.completed,job.amount))} }
                    progress {max:job.amount.to_string(),value:job.completed.to_string(),aria_label:tr("処理の進捗")}
                }
                if job.busy {p {{tr("実行中はアプリを閉じないでください。別の画面を閲覧しても処理は継続します。")}}}
                match &job.outcome {
                    Some(Ok(Some(report))) => rsx!{p {{bilingual(format!("選手{}人を作成し、監督{}人を配置しました。",report.players,report.managers),format!("Generated {} players; assigned {} managers.",report.players,report.managers))}}},
                    Some(Err(error)) => rsx!{p {"{error}"} p {{tr("途中まで保存された結果は保持されます。進捗を確認してから再実行してください。")}}},
                    _ => rsx!{},
                }
            }
        }
        div {class:"management-grid",
            for operation in Operation::ALL { OperationCard {operation,busy:job.busy} }
        }
    }
}

#[component]
fn OperationCard(operation: Operation, busy: bool) -> Element {
    let mut amount = use_signal(|| if operation == Operation::Generate {"100".to_string()} else {"1".to_string()});
    let controller = use_context::<Controller>();
    let valid = operation.parse(&amount());
    let id = operation.id();
    rsx! {
        section {class:"panel management-card",
            h2 {{tr(operation.title())}}
            p {{tr(operation.description())}}
            label {r#for:format!("manage-{id}-amount"),{tr(operation.unit())}}
            input {id:format!("manage-{id}-amount"),r#type:"number",min:operation.min().to_string(),max:operation.max().to_string(),step:"1",value:amount,disabled:busy,
                oninput:move |event|amount.set(event.value())}
            if valid.is_none() {p {class:"validation",role:"alert",{bilingual(format!("{}〜{}の整数を入力してください。",operation.min(),operation.max()),format!("Enter a whole number from {} to {}.",operation.min(),operation.max()))}}}
            button {id:format!("manage-{id}-run"),class:"primary",disabled:busy || valid.is_none(),
                onclick:move |_| { if let Some(n)=operation.parse(&amount()) { controller.submit(operation,n); } },
                {tr("実行")}}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_match_tui_limits_and_reject_fractional_or_invalid_values() {
        assert_eq!(Operation::Generate.parse("0"), Some(0));
        assert_eq!(Operation::Schedule.parse("255"), Some(255));
        assert_eq!(Operation::Schedule.parse("256"), None);
        for operation in Operation::ALL {
            for invalid in ["", "-1", "1.5", "65536", "abc"] { assert_eq!(operation.parse(invalid), None); }
        }
        for operation in [Operation::Schedule, Operation::Process, Operation::Analyze] {assert_eq!(operation.parse("0"), None);}
    }
}
