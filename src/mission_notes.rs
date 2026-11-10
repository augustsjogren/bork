//! Notes that tell mission control what its member cards did: column moves
//! nobody on the mission's side made, a new reason the card needs someone, a
//! merged PR. Every note goes to the mission's `plans/<id>/notes.md`; when it
//! is safe to type into mission control's pane, bork also types a one-line
//! nudge pointing at the file.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::app::Project;
use crate::attention::Attention;
use crate::types::{AgentStatus, Column, IssueKind, MergeQueueState, PrState};

/// Shortest gap between two nudges to one mission control.
pub const NUDGE_INTERVAL: Duration = Duration::from_secs(3 * 60);

#[derive(Debug, Default)]
pub struct MissionNotes {
    /// How each member card looked at the last check, by card id.
    snapshots: HashMap<String, MemberSnapshot>,
    /// Cards the user moved with TUI keys (or archived) since the last check.
    pub user_moved: HashSet<String>,
    /// Notes not yet written to their mission's notes file, by mission id.
    unwritten: HashMap<String, Vec<String>>,
    /// Notes mission control hasn't been nudged about yet, by mission id.
    unread: HashMap<String, usize>,
    nudged_at: HashMap<String, Instant>,
}

#[derive(Debug, Clone)]
struct MemberSnapshot {
    column: Column,
    /// Attention states already noted since the card last changed column, so
    /// one flapping between two states is noted once.
    reported: Vec<Attention>,
    merged_reported: bool,
}

/// What one delivery tick hands to the background writer.
#[derive(Debug, Default, PartialEq)]
pub struct DueNotes {
    /// Notes to append, by mission id.
    pub writes: Vec<(String, Vec<String>)>,
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
        if !self.live.pr_poll_done {
            return;
        }

        let mut seen = HashSet::new();
        let mut notes: Vec<(String, String)> = Vec::new();
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
            let card = match pr {
                Some(pr) => format!("{} (#{})", issue.id, pr.number),
                None => issue.id.clone(),
            };
            let mission_control_busy = self.resolved_agent_status(mission) == AgentStatus::Busy;
            let moved_by_user = self.mission_notes.user_moved.contains(&issue.id);
            let Some(snapshot) = self.mission_notes.snapshots.get_mut(&issue.id) else {
                self.mission_notes.snapshots.insert(
                    issue.id.clone(),
                    MemberSnapshot {
                        column,
                        reported: attention.into_iter().collect(),
                        merged_reported: merged,
                    },
                );
                continue;
            };

            if column != snapshot.column {
                if !moved_by_user && !mission_control_busy {
                    notes.push((mission.id.clone(), format!("{card} moved to {column}")));
                }
                snapshot.column = column;
                snapshot.reported.clear();
            }
            if let Some(attention) = attention.filter(|a| !snapshot.reported.contains(a)) {
                notes.push((
                    mission.id.clone(),
                    format!("{card}: {}", attention_note(attention)),
                ));
                snapshot.reported.push(attention);
            }
            if merged && !snapshot.merged_reported {
                notes.push((mission.id.clone(), format!("{card}: PR merged")));
                snapshot.merged_reported = true;
            }
        }

        self.mission_notes
            .snapshots
            .retain(|id, _| seen.contains(id));
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

    /// Notes waiting for mission control to be nudged about them.
    pub fn unread_mission_notes(&self, mission_id: &str) -> usize {
        self.mission_notes
            .unread
            .get(mission_id)
            .copied()
            .unwrap_or(0)
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
}

/// One line, since it is typed into the agent's input.
pub fn nudge_text(mission_id: &str) -> String {
    format!(
        "[bork] New notes about your cards in {}/{mission_id}/{}. Read them, record what matters in your plan and tell the user; don't act on them unless they ask.",
        crate::backup::PLANS_DIR,
        crate::backup::MISSION_NOTES_NAME
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
