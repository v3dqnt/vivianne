//! What specialises the base agent. Every agent runs the same loop with the
//! same tools; a profile says which of those tools it has and what holds it
//! back. `Gate::decide` (`agent::tools::exec`) enforces it on every call.
//!
//! | Agent | Tools | Asks before acting | Plans only | Delegates |
//! |---|---|---|---|---|
//! | the chat | all (less its mode's and Settings') | yes, unless bypass | when switched on | yes |
//! | `viv run`, Viv Instant | all, or the set given | no one to ask: runs unattended | no | no |
//! | a subagent | its profile's, or all | as its parent: a bot's rules, unattended | as its parent | an employee's own helpers |
//! | a bot (teammate, General agent) | its picks, plus its email and phone | its own rules, above bypass | no | yes |
//!
//! `subagent` holds the saved and built-in subagent profiles.

pub mod subagent;

use crate::agent::runtime::state::AgentState;
use crate::agent::runtime::types::PersonaMode;
use crate::agent::team::bots::BotPolicy;

/// The parts of an agent that decide what it may do.
#[derive(Debug, Clone, Default)]
pub struct AgentProfile {
    /// Ask, Debug and Explore withhold the tools that change things.
    pub persona: PersonaMode,
    /// `None`: every tool. `Some`: only these.
    pub tools: Option<Vec<String>>,
    /// Research and conversation only, until a plan is approved.
    pub plan_mode: bool,
    /// May run subagents of its own (an employee, a bot).
    pub delegation: bool,
    /// The paths it owns in a team; it may write only there.
    pub owned: Vec<String>,
    /// A bot's own rules for what it does alone, asks about, or hands over.
    pub bot: Option<BotPolicy>,
    /// Nobody is watching: approvals are skipped (secrets and paid jobs
    /// still ask) and Viv Instant's risk gate applies.
    pub unattended: bool,
    /// The team run and member name it works as, if any.
    pub team_member: Option<(String, String)>,
}

impl AgentProfile {
    /// Run from the command line or by voice: nobody at the keyboard.
    pub fn unattended(tools: Option<Vec<String>>) -> Self {
        Self { tools, unattended: true, ..Self::default() }
    }

    /// A teammate or General agent, on its own rules.
    pub fn bot(policy: BotPolicy, tools: Option<Vec<String>>) -> Self {
        Self { tools, delegation: true, bot: Some(policy), ..Self::default() }
    }

    /// A delegated task. It keeps what binds its parent: plan mode, a bot's
    /// rules, and running unattended, so delegating can't get round them.
    pub fn subagent(
        parent: &AgentProfile,
        tools: Option<Vec<String>>,
        owned: Vec<String>,
        delegation: bool,
        team_member: Option<(String, String)>,
    ) -> Self {
        Self {
            persona: PersonaMode::Agent,
            tools,
            plan_mode: parent.plan_mode,
            delegation,
            owned,
            bot: parent.bot.clone(),
            unattended: parent.unattended,
            team_member,
        }
    }

    /// The profile an agent is running with.
    pub async fn of(state: &AgentState) -> Self {
        Self {
            persona: state.persona_mode().await,
            tools: state.subagent_allowed_tools().await,
            plan_mode: state.plan_mode().await,
            delegation: state.delegation_allowed().await,
            owned: state.owned_paths().await,
            bot: state.bot_policy().await,
            unattended: state.auto_approve().await,
            team_member: state.team_member().await,
        }
    }

    /// Give an agent this profile, replacing whatever it had.
    pub async fn apply(self, state: &AgentState) {
        state.set_persona_mode(self.persona).await;
        state.set_subagent_allowed_tools(self.tools).await;
        state.set_plan_mode(self.plan_mode).await;
        state.set_delegation_allowed(self.delegation).await;
        state.set_owned_paths(self.owned).await;
        state.set_bot_policy(self.bot).await;
        state.set_auto_approve(self.unattended).await;
        state.set_team_member(self.team_member).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::team::bots::{BotRules, Origin};

    fn policy() -> BotPolicy {
        BotPolicy {
            bot_name: "Goa".into(),
            rules: BotRules::default(),
            origin: Origin::User,
            request: String::new(),
            root: ".".into(),
            bot_id: "goa".into(),
        }
    }

    #[test]
    fn a_subagent_keeps_what_binds_its_parent() {
        let bot = AgentProfile { plan_mode: true, ..AgentProfile::bot(policy(), None) };
        let sub = AgentProfile::subagent(&bot, Some(vec!["read".into()]), vec![], false, None);
        assert!(sub.bot.is_some(), "a bot's rules hold for the work it delegates");
        assert!(sub.plan_mode);
        assert!(!sub.delegation);
        assert_eq!(sub.tools, Some(vec!["read".to_string()]));

        let run = AgentProfile::subagent(&AgentProfile::unattended(None), None, vec![], false, None);
        assert!(run.unattended, "a subagent of an unattended run has nobody to ask either");
        assert!(!AgentProfile::subagent(&AgentProfile::default(), None, vec![], false, None).unattended);
    }

    #[tokio::test]
    async fn apply_then_of_round_trips() {
        let state = AgentState::new();
        let p = AgentProfile { owned: vec!["src/ui".into()], team_member: Some(("r".into(), "m".into())), ..AgentProfile::bot(policy(), Some(vec!["web_search".into()])) };
        p.clone().apply(&state).await;
        let back = AgentProfile::of(&state).await;
        assert_eq!(back.tools, p.tools);
        assert_eq!(back.owned, p.owned);
        assert_eq!(back.team_member, p.team_member);
        assert!(back.delegation && back.bot.is_some() && !back.unattended);
    }
}
