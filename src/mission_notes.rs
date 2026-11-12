//! Notes that tell mission control what its member cards did: column moves
//! nobody on the mission's side made, a new reason the card needs someone, a
//! merged PR. Every note goes to the mission's `plans/<id>/notes.md`; when it
//! is safe to type into mission control's pane, bork also types a one-line
//! nudge pointing at the file. Next to it, `cards.md` lists every member as
//! the board sees it now, rewritten whenever that changes.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::app::Project;
use crate::attention::Attention;
use crate::types::{AgentStatus, Column, Issue, IssueKind, MergeQueueState, PrState};

/// Shortest stretch of work after which an agent going idle is noted, so
/// quick replies don't each make a note.
pub const LONG_RUN: Duration = Duration::from_secs(5 * 60);

/// Shortest gap between two nudges to one mission control.
pub const NUDGE_INTERVAL: Duration = Duration::from_secs(3 * 60);

#[derive(Debug, Default)]
pub struct MissionNotes {
    /// How each member card looked at the last check, by card id.
    snapshots: HashMap<String, MemberSnapshot>,
    /// Cards the user moved with TUI keys (or archived) since the last check.
    pub user_moved: HashSet<String>,
    /// Notes not yet written to their mission's notes file, by mission id.
    unwritten: HashMap<String, Vec<Note>>,
    /// Notes mission control hasn't been nudged about yet, by mission id.
    unread: HashMap<String, usize>,
    nudged_at: HashMap<String, Instant>,
    /// The cards file last handed out per mission, to skip unchanged rewrites.
    cards_written: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct MemberSnapshot {
    mission_id: String,
    /// The card as notes name it, kept for the note when it leaves.
    card: String,
    pr_number: Option<u32>,
    column: Column,
    /// Attention states already noted since the card last changed column, so
    /// one flapping between two states is noted once.
    reported: Vec<Attention>,
    merged_reported: bool,
    /// When the card's agent was first seen working in its current run, in
    /// unix seconds. Kept through permission prompts, which pause the run.
    busy_since: Option<u64>,
}

/// One line for a mission's notes file.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub text: String,
    /// A PR whose final state the writer looks up and appends, for a card
    /// that left the board before its merge or close was seen.
    pub pr_to_check: Option<u32>,
}

impl From<String> for Note {
    fn from(text: String) -> Self {
        Note {
            text,
            pr_to_check: None,
        }
    }
}

impl Note {
    /// The note as written, given the checked PR's state when there is one.
    pub fn resolved(&self, pr_state: Option<PrState>) -> String {
        match pr_state {
            Some(state) if self.pr_to_check.is_some() && state != PrState::Open => {
                format!("{}, PR {state}", self.text)
            }
            _ => self.text.clone(),
        }
    }
}

/// What one delivery tick hands to the background writer.
#[derive(Debug, Default, PartialEq)]
pub struct DueNotes {
    /// Notes to append, by mission id.
    pub writes: Vec<(String, Vec<Note>)>,
    pub nudges: Vec<Nudge>,
}

#[derive(Debug, PartialEq)]
pub struct Nudge {
    pub mission_id: String,
    pub session_name: String,
}

impl Project {
    /// Compare every member card with its last snapshot and queue a note per
    /// change. A card seen for the first time only records its snapshot, so
    /// starting the TUI or joining a mission notes nothing. A move made while
    /// mission control is mid-turn is taken to be its own and not noted back.
    pub fn collect_mission_notes(&mut self) {
        self.collect_mission_notes_at(crate::app::unix_now());
    }

    pub fn collect_mission_notes_at(&mut self, now: u64) {
        if !self.live.pr_poll_done {
            return;
        }

        let mut seen = HashSet::new();
        let mut notes: Vec<(String, Note)> = Vec::new();
        for issue in &self.issues {
            let Some(mission) = crate::ops::mission_of(&self.issues, issue) else {
                continue;
            };
            if mission.mission_notes_off {
                continue;
            }
            seen.insert(issue.id.clone());

            let column = issue.column;
            let attention = self.attention_for(issue).filter(|a| a.needs_you());
            let pr = self.pr_for(issue);
            let merged = pr.is_some_and(|pr| pr.state == PrState::Merged);
            let pr_number = pr.map(|pr| pr.number);
            let card = match pr {
                Some(pr) => format!("{} (#{})", issue.id, pr.number),
                None => issue.id.clone(),
            };
            let mission_control_busy = self.resolved_agent_status(mission) == AgentStatus::Busy;
            let moved_by_user = self.mission_notes.user_moved.contains(&issue.id);
            let agent_status = self.resolved_agent_status(issue);
            let first_seen = MemberSnapshot {
                mission_id: mission.id.clone(),
                card: card.clone(),
                pr_number,
                column,
                reported: attention.into_iter().collect(),
                merged_reported: merged,
                busy_since: (agent_status == AgentStatus::Busy).then_some(now),
            };
            let Some(snapshot) = self.mission_notes.snapshots.get_mut(&issue.id) else {
                self.mission_notes
                    .snapshots
                    .insert(issue.id.clone(), first_seen);
                continue;
            };
            if snapshot.mission_id != mission.id {
                notes.push((
                    snapshot.mission_id.clone(),
                    format!("{} left the mission", snapshot.card).into(),
                ));
                *snapshot = first_seen;
                continue;
            }
            snapshot.card = card.clone();
            if snapshot.pr_number.is_none() && pr_number.is_some() {
                notes.push((mission.id.clone(), format!("{card}: PR opened").into()));
            }
            snapshot.pr_number = pr_number.or(snapshot.pr_number);
            if let Some(minutes) = finished_run(snapshot, agent_status, now) {
                notes.push((
                    mission.id.clone(),
                    format!("{card}: agent finished after {minutes} min").into(),
                ));
            }

            if column != snapshot.column {
                if !moved_by_user && !mission_control_busy {
                    notes.push((
                        mission.id.clone(),
                        format!("{card} moved to {column}").into(),
                    ));
                }
                snapshot.column = column;
                snapshot.reported.clear();
            }
            if let Some(attention) = attention.filter(|a| !snapshot.reported.contains(a)) {
                notes.push((
                    mission.id.clone(),
                    format!("{card}: {}", attention_note(attention)).into(),
                ));
                snapshot.reported.push(attention);
            }
            if merged && !snapshot.merged_reported {
                notes.push((mission.id.clone(), format!("{card}: PR merged").into()));
                snapshot.merged_reported = true;
            }
        }

        let issues = &self.issues;
        self.mission_notes.snapshots.retain(|id, snapshot| {
            if seen.contains(id) {
                return true;
            }
            let note = if issues.iter().any(|issue| issue.id == *id) {
                format!("{} left the mission", snapshot.card).into()
            } else {
                Note {
                    text: format!("{}: card removed from the board", snapshot.card),
                    pr_to_check: snapshot.pr_number.filter(|_| !snapshot.merged_reported),
                }
            };
            notes.push((snapshot.mission_id.clone(), note));
            false
        });
        self.mission_notes.user_moved.clear();
        for (mission_id, note) in notes {
            *self
                .mission_notes
                .unread
                .entry(mission_id.clone())
                .or_default() += 1;
            self.mission_notes
                .unwritten
                .entry(mission_id)
                .or_default()
                .push(note);
        }
    }

    /// Hand out every unwritten note, and a nudge for each mission whose
    /// mission control just finished a turn (its status hook reported Idle,
    /// which no approval prompt does) and wasn't nudged within
    /// `NUDGE_INTERVAL`. Muted and vanished missions are dropped.
    pub fn take_due_mission_notes(&mut self, now: Instant) -> DueNotes {
        let live_mission = |id: &str| {
            self.issues.iter().find(|issue| {
                issue.id == id && issue.kind == IssueKind::Orchestrator && !issue.mission_notes_off
            })
        };
        let gone: Vec<String> = self
            .mission_notes
            .unread
            .keys()
            .chain(self.mission_notes.unwritten.keys())
            .filter(|id| live_mission(id).is_none())
            .cloned()
            .collect();
        for id in gone {
            self.mission_notes.unread.remove(&id);
            self.mission_notes.unwritten.remove(&id);
        }

        let writes = self.mission_notes.unwritten.drain().collect();
        let mut nudges = Vec::new();
        for (mission_id, count) in &mut self.mission_notes.unread {
            let Some(mission) = self.issues.iter().find(|issue| issue.id == *mission_id) else {
                continue;
            };
            let session_name = mission.session_name(&self.config.project_name);
            let just_idle = self.live.active_sessions.contains(&session_name)
                && self
                    .live
                    .agent_statuses
                    .get(&session_name)
                    .is_some_and(|info| info.status == AgentStatus::Idle);
            let recently_nudged = self
                .mission_notes
                .nudged_at
                .get(mission_id)
                .is_some_and(|at| now.duration_since(*at) < NUDGE_INTERVAL);
            if *count == 0 || !just_idle || recently_nudged {
                continue;
            }
            *count = 0;
            self.mission_notes.nudged_at.insert(mission_id.clone(), now);
            nudges.push(Nudge {
                mission_id: mission_id.clone(),
                session_name,
            });
        }
        self.mission_notes.unread.retain(|_, count| *count > 0);
        DueNotes { writes, nudges }
    }

    /// The cards file of every open mission whose content changed since it
    /// was last handed out, by mission id.
    pub fn take_changed_mission_cards(&mut self) -> Vec<(String, String)> {
        if !self.live.pr_poll_done {
            return Vec::new();
        }
        let current: Vec<(String, String)> = self
            .open_missions()
            .map(|mission| (mission.id.clone(), self.mission_cards(&mission.id)))
            .collect();
        let mut changed = Vec::new();
        for (mission_id, cards) in current {
            if self.mission_notes.cards_written.get(&mission_id) == Some(&cards) {
                continue;
            }
            self.mission_notes
                .cards_written
                .insert(mission_id.clone(), cards.clone());
            changed.push((mission_id, cards));
        }
        changed
    }

    fn mission_cards(&self, mission_id: &str) -> String {
        let mut out = format!(
            "# Cards in {mission_id}\n\nWritten by bork whenever a card changes; edits are overwritten.\n\n"
        );
        let mut members: Vec<&Issue> = self
            .issues
            .iter()
            .filter(|issue| crate::ops::is_member_of(issue, mission_id))
            .collect();
        if members.is_empty() {
            out.push_str("No cards yet.\n");
            return out;
        }
        members.sort_by_key(|issue| issue.column.index());

        out.push_str("| Card | Column | PR | Needs | Title |\n| --- | --- | --- | --- | --- |\n");
        for issue in members {
            let pr = match self.pr_for(issue) {
                Some(pr) if pr.state != PrState::Open => format!("#{} {}", pr.number, pr.state),
                Some(pr) if pr.is_draft => format!("#{} draft", pr.number),
                Some(pr) => format!("#{}", pr.number),
                None => String::new(),
            };
            let needs = self
                .attention_for(issue)
                .filter(|attention| attention.needs_you())
                .map(attention_note)
                .unwrap_or_default();
            out.push_str(&format!(
                "| {} | {} | {pr} | {needs} | {} |\n",
                issue.id,
                issue.column,
                issue.title.replace('|', "\\|"),
            ));
        }
        out
    }
}

/// Track the card's agent run, returning its length in minutes when a run of
/// at least `LONG_RUN` just ended with the agent idle.
fn finished_run(snapshot: &mut MemberSnapshot, status: AgentStatus, now: u64) -> Option<u64> {
    match status {
        AgentStatus::Busy => {
            snapshot.busy_since.get_or_insert(now);
            None
        }
        AgentStatus::WaitingPermission | AgentStatus::WaitingApproval => None,
        AgentStatus::Idle | AgentStatus::WaitingInput => {
            let ran = now.saturating_sub(snapshot.busy_since.take()?);
            (ran >= LONG_RUN.as_secs()).then_some(ran / 60)
        }
        AgentStatus::Stopped | AgentStatus::Error => {
            snapshot.busy_since = None;
            None
        }
    }
}

/// One line, since it is typed into the agent's input.
pub fn nudge_text(mission_id: &str) -> String {
    format!(
        "[bork] New notes about your cards in {plans}/{mission_id}/{}. Read them, record what matters in your plan and tell the user; don't act on them unless they ask. {plans}/{mission_id}/{} always lists the mission's cards as they are now.",
        crate::backup::MISSION_NOTES_NAME,
        crate::backup::MISSION_CARDS_NAME,
        plans = crate::backup::PLANS_DIR,
    )
}

fn attention_note(attention: Attention) -> &'static str {
    match attention {
        Attention::ReadyToMerge => "PR ready to merge",
        Attention::ChecksFailing => "checks failing",
        Attention::ChangesRequested => "changes requested",
        Attention::ChecksAndChanges => "checks failing and changes requested",
        Attention::NeedsRebase => "PR needs a rebase",
        Attention::NeedsUpdate => "PR needs updating with its base",
        Attention::Blocked => "PR approved and green, but GitHub blocks the merge",
        Attention::Queue(MergeQueueState::Failed) => "merge queue failed",
        Attention::Queue(MergeQueueState::Cancelled) => "dequeued from the merge queue",
        // Not a needs-you state, so never noted.
        Attention::Queue(MergeQueueState::Queued | MergeQueueState::Testing) => {
            "in the merge queue"
        }
        Attention::ReviewReady => "review ready to read",
        Attention::ReReview => "review requested again",
        Attention::AgentBlocked => "agent waiting on an approval",
        Attention::AgentError => "agent reported an error",
    }
}
