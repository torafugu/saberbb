use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::batting_personnel::ManagerBattingPolicy;
use crate::repositories::manager_repository::ManagerRepository;
use anyhow::Result;

pub struct ManagerService<R: ManagerRepository> {
    pub repo: R,
}

impl<R: ManagerRepository> ManagerService<R> {
    /// Fill vacancies only; the player generation count does not limit managers.
    pub fn generate_missing_managers(
        &mut self,
        language: &str,
        rng: &mut dyn RandomProvider,
    ) -> Result<usize> {
        use ManagerBattingPolicy::*;
        let policies = [LetBatterDecide, PutBallInPlay, SeekLineDrive, Situational];
        let mut assigned = 0;
        for team_id in self.repo.unmanaged_team_ids()? {
            let name = self.repo.random_name(language)?;
            let full_name = if language.eq_ignore_ascii_case("jp") {
                format!("{} {}", name.last, name.first)
            } else {
                format!("{} {}", name.first, name.last)
            };
            let policy = policies[rng.gen_range(0, policies.len())];
            if self.repo.create_and_assign(team_id, &full_name, policy)?.is_some() {
                assigned += 1;
            }
        }
        Ok(assigned)
    }
}
