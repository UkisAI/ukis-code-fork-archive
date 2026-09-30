//! Native /loop UI and submission through the ordinary permission-preserving turn path.
use super::*;
use crate::loop_command::LoopCommand;
use crate::loop_scheduler::Cadence;
use crate::loop_scheduler::Decision;
use crate::loop_scheduler::MAX_PROMPT_BYTES;
use serde::Deserialize;
use std::io::Read;

#[derive(Clone, Copy)]
pub(crate) enum LoopAvailability {
    Adaptive,
    FixedOnly,
    Disconnected,
}

const HELP: &str = "/loop [interval] [prompt]  Start a loop (s, m, h, d; minimum 1 minute)\n/loop <prompt>           Adaptive: the model chooses each delay\n/loop                    Adaptive maintenance, or your loop.md\n/loop list               Show this conversation's loops\n/loop stop <id>           Cancel one loop\n/loop stop               Cancel all loops\n\nLoops use the current model, effort, and permissions. They wait for idle, expire after 7 days, and stop when you leave this conversation or close Ukis. Esc stops adaptive loops when the composer is empty.";
const MAINTENANCE: &str = "Continue unfinished work already requested in this conversation. If none remains, check the current branch's pull request for failed CI, review comments, or merge conflicts. If that is clear, look for small, relevant bugs or simplifications. Stay within the existing task scope; do not start unrelated initiatives. Irreversible actions, including pushing or deleting, require authorization already present in the conversation. If there is nothing useful to do, report that briefly.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WakeupRequest {
    loop_id: u64,
    run_id: String,
    delay_seconds: Option<u64>,
    stop: Option<bool>,
    reason: String,
}

impl ChatWidget {
    pub(crate) fn handle_loop_command(
        &mut self,
        args: &str,
        now: Instant,
        availability: LoopAvailability,
    ) {
        let result = crate::loop_command::parse(args).and_then(|command| {
            match command {
                LoopCommand::Help => self.add_info_message(HELP.into(), None),
                LoopCommand::List => {
                    let mut lines = vec![Line::from("Loops in this conversation".bold())];
                    for task in &self.loop_scheduler.tasks {
                        let cadence = match task.cadence {
                            Cadence::Fixed(interval) => format!("every {}m", interval.as_secs() / 60),
                            Cadence::Adaptive => "adaptive".to_owned(),
                        };
                        let seconds = task.due.saturating_duration_since(now).as_secs();
                        let preview: String = task.prompt.as_deref().unwrap_or("loop.md / maintenance")
                            .chars().take(160).map(|ch| if ch.is_control() { ' ' } else { ch }).collect();
                        lines.push(Line::from(format!("{}: {cadence}; due in {seconds}s; {preview}", task.id)));
                    }
                    if self.loop_scheduler.tasks.is_empty() {
                        lines.push(Line::from("No active loops."));
                    }
                    lines.push(Line::from("/loop stop <id> or /loop stop all".dim()));
                    self.add_plain_history_lines(lines);
                }
                LoopCommand::StopAll => {
                    let count = self.loop_scheduler.cancel_all();
                    self.add_info_message(format!("Stopped {count} loop(s)."), Some("Future runs are cancelled. An iteration already running can be interrupted with Esc.".into()));
                }
                LoopCommand::Stop(id) => {
                    if !self.loop_scheduler.cancel(id) {
                        return Err(format!("No active loop {id}. Use /loop list."));
                    }
                    self.add_info_message(format!("Stopped loop {id}."), Some("Future runs are cancelled; an iteration already running is not interrupted.".into()));
                }
                LoopCommand::Start { cadence, prompt } => {
                    if matches!(availability, LoopAvailability::Disconnected) {
                        return Err("Reconnect before starting a loop. Existing loops can still be listed or stopped.".into());
                    }
                    if cadence == Cadence::Adaptive && matches!(availability, LoopAvailability::FixedOnly) {
                        return Err("Adaptive loop controls are unavailable on this connection. Use an explicit interval, such as /loop 5m check CI.".into());
                    }
                    if !self.is_session_configured() {
                        return Err("Wait for this conversation to finish connecting before starting a loop.".into());
                    }
                    // Validate defaults now, but reload them on every iteration.
                    if prompt.is_none() {
                        self.default_loop_prompt()?;
                    }
                    let id = self.loop_scheduler.add(prompt, cadence, now)?;
                    let schedule = match cadence {
                        Cadence::Fixed(interval) => format!("every {} minute(s); first run after that interval", interval.as_secs() / 60),
                        Cadence::Adaptive => "adaptive; first run when idle, then model-selected delays of 1-60 minutes".into(),
                    };
                    self.add_info_message(format!("Loop {id} scheduled: {schedule}."), Some("Use /loop list or /loop stop. Stops on conversation change or exit; expires after 7 days.".into()));
                }
            }
            Ok(())
        });
        if let Err(error) = result {
            self.add_error_message(error);
        }
        self.request_redraw();
    }

    pub(crate) fn has_loop_tasks(&self) -> bool {
        !self.loop_scheduler.tasks.is_empty()
    }

    pub(crate) fn poll_loops(&mut self, now: Instant) {
        for id in self.loop_scheduler.expire(now) {
            self.add_info_message(format!("Loop {id} expired after 7 days."), None);
        }
        if !self.is_session_configured()
            || self.mcp_startup_status.is_some()
            || self.bottom_pane.questions.is_some()
            || self.blocks_direct_input
            || self.fork_in_progress
            || self.active_side_conversation
            || self.has_misalignment_policy_violation()
            || self.is_user_turn_pending_or_running()
            || self.is_plan_streaming_in_tui()
            || self.input_queue.suppress_queue_autosend
            || self.input_queue.rate_limit_recovery_pending
            || self.input_queue.recovered_queue
            || self.input_queue.has_unconfirmed_messages()
            || !self.input_queue.queued_user_messages.is_empty()
            || !self.input_queue.rejected_steers_queue.is_empty()
            || !self.bottom_pane.no_modal_or_popup_active()
            || !self.bottom_pane.composer_text().is_empty()
            || !self.bottom_pane.composer_local_images().is_empty()
            || !self.bottom_pane.remote_image_urls().is_empty()
            || self.external_editor_state != ExternalEditorState::Closed
        {
            return;
        }
        let Some(fire) = self.loop_scheduler.take_due(now) else {
            return;
        };
        let prompt = match fire.prompt {
            Some(prompt) => Ok(prompt),
            None => self.default_loop_prompt(),
        };
        let mut prompt = match prompt {
            Ok(prompt) => prompt,
            Err(error) => {
                self.stop_active_loop();
                self.add_error_message(error);
                return;
            }
        };
        if fire.cadence == Cadence::Adaptive && !fire.final_run {
            prompt.push_str(&format!(
                "\n\n[Ukis adaptive loop {} - iteration {}]\nAt the end of this iteration, call the ukis_loop schedule_wakeup tool with loop_id {}, run_id \"{}\", and a short reason. Choose delay_seconds from 60 to 3600 based on what you observed, or stop:true when the task is complete. Keep the existing task scope and permissions. Do not create another loop.",
                fire.task_id, fire.run_id, fire.task_id, fire.run_id,
            ));
        }
        let label = if fire.final_run {
            "Final run of expired loop"
        } else {
            "Running loop"
        };
        self.add_info_message(format!("{label} {}.", fire.task_id), /*hint*/ None);
        if self
            .submit_user_message_with_shell_escape_policy(
                UserMessage::from(prompt),
                ShellEscapePolicy::Disallow,
            )
            .is_none()
        {
            self.stop_active_loop();
        }
    }

    pub(crate) fn handle_loop_tool(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        arguments: serde_json::Value,
    ) -> Result<String, String> {
        if self.thread_id.map(|id| id.to_string()).as_deref() != Some(thread_id) {
            return Err("This loop belongs to a different conversation.".into());
        }
        let request: WakeupRequest =
            serde_json::from_value(arguments).map_err(|error| error.to_string())?;
        let reason = request.reason.trim();
        if reason.is_empty() || reason.chars().count() > 500 || reason.chars().any(char::is_control)
        {
            return Err("Provide a single-line reason of 1-500 characters.".into());
        }
        let run_id = uuid::Uuid::parse_str(&request.run_id).map_err(|error| error.to_string())?;
        let (decision, message) = match (request.stop, request.delay_seconds) {
            (Some(true), None) => (
                Decision::Stop,
                format!("Loop {} stopped: {reason}", request.loop_id),
            ),
            (None | Some(false), Some(seconds)) => (
                Decision::Wait(Duration::from_secs(seconds)),
                format!(
                    "Loop {}: next run {seconds}s after this iteration. {reason}",
                    request.loop_id
                ),
            ),
            _ => return Err("Supply either delay_seconds or stop:true, not both.".into()),
        };
        self.loop_scheduler
            .decide(request.loop_id, run_id, turn_id, decision)?;
        self.add_info_message(message.clone(), None);
        Ok(message)
    }

    pub(crate) fn stop_active_loop(&mut self) {
        if let Some(id) = self.loop_scheduler.cancel_active() {
            self.add_info_message(
                format!("Loop {id} stopped after an interrupted or unsuccessful iteration."),
                None,
            );
        }
    }

    fn default_loop_prompt(&self) -> Result<String, String> {
        let paths = [
            self.config.cwd.join(".ukis").join("loop.md"),
            self.config.codex_home.join("loop.md"),
        ];
        for path in paths {
            let file = match std::fs::File::open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(format!("Cannot read {}: {error}", path.display())),
            };
            let mut text = String::new();
            file.take(MAX_PROMPT_BYTES as u64 + 1)
                .read_to_string(&mut text)
                .map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
            if text.trim().is_empty() || text.len() > MAX_PROMPT_BYTES {
                return Err(format!(
                    "{} must contain 1-{MAX_PROMPT_BYTES} UTF-8 bytes.",
                    path.display()
                ));
            }
            return Ok(text);
        }
        Ok(MAINTENANCE.into())
    }
}
