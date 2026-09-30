use super::*;
use pretty_assertions::assert_eq;

#[test]
fn fixed_schedule_coalesces_missed_intervals_without_overlapping() {
    let now = Instant::now();
    let mut scheduler = LoopScheduler::default();
    let id = scheduler
        .add(
            Some("check".into()),
            Cadence::Fixed(Duration::from_secs(60)),
            now,
        )
        .unwrap();
    assert!(scheduler.take_due(now + Duration::from_secs(59)).is_none());
    let fire = scheduler.take_due(now + Duration::from_secs(600)).unwrap();
    assert_eq!((fire.task_id, fire.prompt), (id, Some("check".into())));
    assert!(scheduler.take_due(now + Duration::from_secs(600)).is_none());
    scheduler.bind_turn("turn");
    scheduler.finish(now + Duration::from_secs(601));
    assert!(scheduler.take_due(now + Duration::from_secs(601)).is_none());
    assert_eq!(
        scheduler
            .take_due(now + Duration::from_secs(660))
            .unwrap()
            .task_id,
        id
    );
}

#[test]
fn adaptive_delay_starts_after_completion_and_rejects_stale_decisions() {
    let now = Instant::now();
    let mut scheduler = LoopScheduler::default();
    let id = scheduler
        .add(Some("check".into()), Cadence::Adaptive, now)
        .unwrap();
    let fire = scheduler.take_due(now).unwrap();
    scheduler.bind_turn("turn-1");
    assert!(
        scheduler
            .decide(
                id,
                fire.run_id,
                "wrong-turn",
                Decision::Wait(Duration::from_secs(60))
            )
            .is_err()
    );
    scheduler
        .decide(
            id,
            fire.run_id,
            "turn-1",
            Decision::Wait(Duration::from_secs(120)),
        )
        .unwrap();
    assert!(
        scheduler
            .decide(
                id,
                fire.run_id,
                "turn-1",
                Decision::Wait(Duration::from_secs(120))
            )
            .is_err()
    );
    assert_eq!(scheduler.finish(now + Duration::from_secs(10)), None);
    assert!(scheduler.take_due(now + Duration::from_secs(129)).is_none());
    let next = scheduler.take_due(now + Duration::from_secs(130)).unwrap();
    scheduler.bind_turn("turn-2");
    assert_ne!(fire.run_id, next.run_id);
    assert!(
        scheduler
            .decide(id, fire.run_id, "turn-1", Decision::Stop)
            .is_err()
    );
    scheduler
        .decide(id, next.run_id, "turn-2", Decision::Stop)
        .unwrap();
    assert!(scheduler.tasks.is_empty());
}

#[test]
fn cancellation_wins_over_pending_and_late_wakeups() {
    let now = Instant::now();
    let mut scheduler = LoopScheduler::default();
    let id = scheduler
        .add(/*prompt*/ None, Cadence::Adaptive, now)
        .unwrap();
    let fire = scheduler.take_due(now).unwrap();
    scheduler.bind_turn("turn");
    scheduler
        .decide(
            id,
            fire.run_id,
            "turn",
            Decision::Wait(Duration::from_secs(60)),
        )
        .unwrap();
    assert!(scheduler.cancel(id));
    assert!(
        scheduler
            .decide(
                id,
                fire.run_id,
                "turn",
                Decision::Wait(Duration::from_secs(60))
            )
            .is_err()
    );
    assert_eq!(scheduler.finish(now), None);
    assert!(scheduler.take_due(now + Duration::from_secs(600)).is_none());
}

#[test]
fn adaptive_missing_decision_gets_only_one_fallback() {
    let now = Instant::now();
    let mut scheduler = LoopScheduler::default();
    scheduler
        .add(/*prompt*/ None, Cadence::Adaptive, now)
        .unwrap();
    scheduler.take_due(now).unwrap();
    assert!(scheduler.finish(now).unwrap().contains("One retry"));
    assert!(
        scheduler
            .take_due(now + Duration::from_secs(1199))
            .is_none()
    );
    scheduler.take_due(now + Duration::from_secs(1200)).unwrap();
    assert!(
        scheduler
            .finish(now + Duration::from_secs(1200))
            .unwrap()
            .contains("stopped")
    );
    assert!(scheduler.tasks.is_empty());
}

#[test]
fn expiry_limits_capacity_and_interrupts_do_not_cancel_other_jobs() {
    let now = Instant::now();
    let mut scheduler = LoopScheduler::default();
    let first = scheduler
        .add(/*prompt*/ None, Cadence::Adaptive, now)
        .unwrap();
    for _ in 1..MAX_TASKS {
        scheduler
            .add(
                /*prompt*/ None,
                Cadence::Fixed(Duration::from_secs(60)),
                now,
            )
            .unwrap();
    }
    assert!(
        scheduler
            .add(/*prompt*/ None, Cadence::Adaptive, now)
            .is_err()
    );
    scheduler.take_due(now).unwrap();
    assert_eq!(scheduler.cancel_active(), Some(first));
    assert_eq!(scheduler.tasks.len(), MAX_TASKS - 1);
    assert!(
        scheduler
            .expire(now + LIFETIME + Duration::from_secs(1))
            .is_empty()
    );
    for _ in 1..MAX_TASKS {
        assert!(
            scheduler
                .take_due(now + LIFETIME + Duration::from_secs(1))
                .unwrap()
                .final_run
        );
        scheduler.finish(now + LIFETIME);
    }
    assert!(scheduler.tasks.is_empty());
    assert!(scheduler.take_due(now + LIFETIME).is_none());
}

#[test]
fn prompts_and_delays_are_bounded_and_stop_overrides_a_delay() {
    let now = Instant::now();
    let mut scheduler = LoopScheduler::default();
    assert!(
        scheduler
            .add(
                Some("x".repeat(MAX_PROMPT_BYTES + 1)),
                Cadence::Adaptive,
                now
            )
            .is_err()
    );
    assert!(
        scheduler
            .add(
                /*prompt*/ None,
                Cadence::Fixed(Duration::from_secs(0)),
                now
            )
            .is_err()
    );
    let id = scheduler
        .add(/*prompt*/ None, Cadence::Adaptive, now)
        .unwrap();
    let fire = scheduler.take_due(now).unwrap();
    scheduler.bind_turn("turn");
    for seconds in [0, 59, 3601, u64::MAX] {
        assert!(
            scheduler
                .decide(
                    id,
                    fire.run_id,
                    "turn",
                    Decision::Wait(Duration::from_secs(seconds))
                )
                .is_err()
        );
    }
    scheduler
        .decide(
            id,
            fire.run_id,
            "turn",
            Decision::Wait(Duration::from_secs(60)),
        )
        .unwrap();
    scheduler
        .decide(id, fire.run_id, "turn", Decision::Stop)
        .unwrap();
    assert_eq!(scheduler.finish(now), None);
    assert!(scheduler.tasks.is_empty());
}

#[test]
fn seven_day_interval_gets_one_final_run_even_if_the_timer_is_late() {
    let now = Instant::now();
    let mut scheduler = LoopScheduler::default();
    scheduler
        .add(/*prompt*/ None, Cadence::Fixed(LIFETIME), now)
        .unwrap();
    let late = now + LIFETIME + Duration::from_secs(2);
    assert!(scheduler.expire(late).is_empty());
    assert!(scheduler.take_due(late).unwrap().final_run);
    scheduler.finish(late);
    assert!(scheduler.take_due(late + LIFETIME).is_none());
}
