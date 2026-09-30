//! Session-local recurring work. The TUI submits at most one ordinary turn at a time.
use std::time::Duration;
use std::time::Instant;
use uuid::Uuid;

pub(crate) const MAX_PROMPT_BYTES: usize = 8_192;
pub(crate) const MAX_TASKS: usize = 50;
pub(crate) const LIFETIME: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cadence {
    Fixed(Duration),
    Adaptive,
}

#[derive(Debug)]
pub(crate) struct LoopTask {
    pub(crate) id: u64,
    pub(crate) prompt: Option<String>,
    pub(crate) cadence: Cadence,
    pub(crate) due: Instant,
    expires: Instant,
    missing_decisions: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Wait(Duration),
    Stop,
}

#[derive(Debug)]
struct ActiveRun {
    task_id: u64,
    run_id: Uuid,
    decision: Option<Decision>,
    turn_id: Option<String>,
}

pub(crate) struct Fire {
    pub(crate) task_id: u64,
    pub(crate) run_id: Uuid,
    pub(crate) prompt: Option<String>,
    pub(crate) cadence: Cadence,
    pub(crate) final_run: bool,
}

#[derive(Default)]
pub(crate) struct LoopScheduler {
    pub(crate) tasks: Vec<LoopTask>,
    active: Option<ActiveRun>,
    next_id: u64,
}

impl LoopScheduler {
    pub(crate) fn add(
        &mut self,
        prompt: Option<String>,
        cadence: Cadence,
        now: Instant,
    ) -> Result<u64, String> {
        if self.tasks.len() >= MAX_TASKS {
            return Err(format!(
                "At most {MAX_TASKS} loops can run in one conversation."
            ));
        }
        if prompt
            .as_ref()
            .is_some_and(|text| text.trim().is_empty() || text.len() > MAX_PROMPT_BYTES)
        {
            return Err(format!(
                "A loop prompt must contain 1 to {MAX_PROMPT_BYTES} UTF-8 bytes."
            ));
        }
        let delay = match cadence {
            Cadence::Fixed(interval)
                if (Duration::from_secs(60)..=LIFETIME).contains(&interval) =>
            {
                interval
            }
            Cadence::Fixed(_) => {
                return Err("Loop intervals must be between 1 minute and 7 days.".into());
            }
            Cadence::Adaptive => Duration::ZERO,
        };
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("Loop ID limit reached.")?;
        self.tasks.push(LoopTask {
            id: self.next_id,
            prompt,
            cadence,
            due: now + delay,
            expires: now + LIFETIME,
            missing_decisions: 0,
        });
        Ok(self.next_id)
    }

    pub(crate) fn cancel(&mut self, id: u64) -> bool {
        let before = self.tasks.len();
        self.tasks.retain(|task| task.id != id);
        before != self.tasks.len()
    }

    pub(crate) fn cancel_all(&mut self) -> usize {
        let count = self.tasks.len();
        self.tasks.clear();
        count
    }

    pub(crate) fn cancel_adaptive(&mut self) -> usize {
        let before = self.tasks.len();
        self.tasks.retain(|task| task.cadence != Cadence::Adaptive);
        before - self.tasks.len()
    }

    pub(crate) fn cancel_active(&mut self) -> Option<u64> {
        let run = self.active.take()?;
        self.cancel(run.task_id).then_some(run.task_id)
    }

    pub(crate) fn expire(&mut self, now: Instant) -> Vec<u64> {
        // An in-flight iteration is already the final run. Otherwise leave one
        // final due fire, including when a seven-day interval wakes slightly late.
        let expired: Vec<_> = self
            .tasks
            .iter()
            .filter(|task| {
                now >= task.expires
                    && self
                        .active
                        .as_ref()
                        .is_some_and(|run| run.task_id == task.id)
            })
            .map(|task| task.id)
            .collect();
        self.tasks.retain(|task| !expired.contains(&task.id));
        expired
    }

    pub(crate) fn take_due(&mut self, now: Instant) -> Option<Fire> {
        if self.active.is_some() {
            return None;
        }
        let task = self
            .tasks
            .iter_mut()
            .filter(|task| task.due.min(task.expires) <= now)
            .min_by_key(|task| (task.due.min(task.expires), task.id))?;
        let run_id = Uuid::new_v4();
        let fire = Fire {
            task_id: task.id,
            run_id,
            prompt: task.prompt.clone(),
            cadence: task.cadence,
            final_run: now >= task.expires,
        };
        if let Cadence::Fixed(interval) = task.cadence {
            task.due = now + interval;
        }
        self.active = Some(ActiveRun {
            task_id: task.id,
            run_id,
            decision: None,
            turn_id: None,
        });
        if fire.final_run {
            self.cancel(fire.task_id);
        }
        Some(fire)
    }

    pub(crate) fn bind_turn(&mut self, turn_id: &str) {
        if let Some(run) = self.active.as_mut()
            && run.turn_id.is_none()
        {
            run.turn_id = Some(turn_id.to_owned());
        }
    }

    pub(crate) fn owns_turn(&self, turn_id: &str) -> bool {
        self.active
            .as_ref()
            .is_some_and(|run| run.turn_id.as_deref() == Some(turn_id))
    }

    pub(crate) fn decide(
        &mut self,
        id: u64,
        run_id: Uuid,
        turn_id: &str,
        decision: Decision,
    ) -> Result<(), String> {
        let task = self
            .tasks
            .iter()
            .find(|task| task.id == id)
            .ok_or("This loop was stopped or expired.")?;
        if task.cadence != Cadence::Adaptive {
            return Err("Fixed loops do not accept adaptive scheduling decisions.".into());
        }
        if let Decision::Wait(delay) = decision
            && !(Duration::from_secs(60)..=Duration::from_secs(3600)).contains(&delay)
        {
            return Err("Choose a delay between 60 and 3600 seconds.".into());
        }
        let run = self
            .active
            .as_mut()
            .filter(|run| {
                run.task_id == id && run.run_id == run_id && run.turn_id.as_deref() == Some(turn_id)
            })
            .ok_or("This is not the current loop iteration.")?;
        if run.decision.is_some() && decision != Decision::Stop {
            return Err("This iteration already scheduled its next wakeup.".into());
        }
        run.decision = Some(decision.clone());
        if decision == Decision::Stop {
            self.cancel(id);
        }
        Ok(())
    }

    /// Called only after the submitted turn and all protected input have settled.
    pub(crate) fn finish(&mut self, now: Instant) -> Option<String> {
        let run = self.active.take()?;
        let task = self.tasks.iter_mut().find(|task| task.id == run.task_id)?;
        if task.cadence != Cadence::Adaptive {
            return None;
        }
        match run.decision {
            Some(Decision::Wait(delay)) => {
                task.due = now + delay;
                task.missing_decisions = 0;
                None
            }
            Some(Decision::Stop) => {
                self.cancel(run.task_id);
                None
            }
            None if task.missing_decisions == 0 => {
                task.missing_decisions = 1;
                task.due = now + Duration::from_secs(20 * 60);
                Some(format!(
                    "Loop {} did not choose its next delay. One retry in 20 minutes; another missing decision stops it.",
                    run.task_id
                ))
            }
            None => {
                self.cancel(run.task_id);
                Some(format!(
                    "Loop {} stopped after two iterations without a scheduling decision.",
                    run.task_id
                ))
            }
        }
    }
}

#[cfg(test)]
#[path = "loop_scheduler_tests.rs"]
mod tests;
