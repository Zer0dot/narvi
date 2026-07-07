//! Day/night scheduler: sun-based (lat/long) or fixed times, with a smooth
//! blend between the day and night profiles over `transition_minutes`.

use std::sync::Arc;

use chrono::{DateTime, Local, NaiveTime, TimeDelta};
use narvi_core::ColorParams;
use narvi_core::config::{ScheduleMode, Scheduling};
use sunrise::{Coordinates, SolarDay, SolarEvent};
use tokio::sync::Mutex;

use crate::state::Daemon;

/// Day boundaries for a given date: (day starts, night starts).
fn boundaries(
    cfg: &Scheduling,
    date: chrono::NaiveDate,
) -> Option<(DateTime<Local>, DateTime<Local>)> {
    match cfg.mode {
        ScheduleMode::Off => None,
        ScheduleMode::Sun => {
            let coord = Coordinates::new(cfg.latitude?, cfg.longitude?)?;
            let day = SolarDay::new(coord, date);
            let rise = day.event_time(SolarEvent::Sunrise).with_timezone(&Local);
            let set = day.event_time(SolarEvent::Sunset).with_timezone(&Local);
            Some((rise, set))
        }
        ScheduleMode::Fixed => {
            let parse = |s: &Option<String>, default: (u32, u32)| {
                s.as_deref()
                    .and_then(|s| NaiveTime::parse_from_str(s, "%H:%M").ok())
                    .unwrap_or_else(|| {
                        NaiveTime::from_hms_opt(default.0, default.1, 0).unwrap_or_default()
                    })
            };
            let day = parse(&cfg.day_time, (8, 0));
            let night = parse(&cfg.night_time, (20, 0));
            Some((
                date.and_time(day).and_local_timezone(Local).single()?,
                date.and_time(night).and_local_timezone(Local).single()?,
            ))
        }
    }
}

/// Night blend at `now`: 0 = day, 1 = night, ramping over the transition
/// window centered on each boundary. Also returns secs until the next ramp.
fn blend_at(cfg: &Scheduling, now: DateTime<Local>) -> Option<(f32, Option<u64>)> {
    let half =
        TimeDelta::seconds(i64::from(cfg.transition_minutes) * 60 / 2).max(TimeDelta::seconds(1));
    let today = boundaries(cfg, now.date_naive())?;
    let tomorrow = boundaries(cfg, now.date_naive() + TimeDelta::days(1))?;

    let ramp = |t: DateTime<Local>| {
        ((now - (t - half)).num_seconds() as f32 / (2 * half.num_seconds()) as f32).clamp(0.0, 1.0)
    };

    let (rise, set) = today;
    // Order: night → [rise ramp] → day → [set ramp] → night.
    let blend = if now < rise - half {
        1.0
    } else if now <= rise + half {
        1.0 - ramp(rise)
    } else if now < set - half {
        0.0
    } else if now <= set + half {
        ramp(set)
    } else {
        1.0
    };

    let next = [rise - half, set - half, tomorrow.0 - half]
        .into_iter()
        .filter(|t| *t > now)
        .map(|t| (t - now).num_seconds() as u64)
        .min();
    Some((blend, next))
}

pub fn spawn(daemon: Arc<Mutex<Daemon>>) {
    tokio::spawn(async move {
        let mut last_blend: Option<f32> = None;
        loop {
            let mut tick = 60u64;
            {
                let mut d = daemon.lock().await;
                let cfg = d.cfg.scheduling.clone();
                let enabled = cfg.enabled && cfg.mode != ScheduleMode::Off;
                let result = if enabled {
                    blend_at(&cfg, Local::now())
                } else {
                    None
                };
                match result {
                    Some((blend, next)) => {
                        d.schedule.mode = format!("{:?}", cfg.mode).to_lowercase();
                        d.schedule.blend = blend;
                        d.schedule.next_transition_secs = next;
                        // Poll fast near/inside a ramp so the blend is smooth.
                        if next.is_some_and(|n| n < 90) || (0.0 < blend && blend < 1.0) {
                            tick = 5;
                        }
                        let moved = last_blend.is_none_or(|b| (b - blend).abs() > 0.001);
                        // Don't fight per-app auto-switch while it holds a profile.
                        if moved && d.auto_class.is_none() {
                            let day = d.cfg.profile(&cfg.day_profile).map(|p| p.params);
                            let night = d.cfg.profile(&cfg.night_profile).map(|p| p.params);
                            if let (Some(day), Some(night)) = (day, night) {
                                last_blend = Some(blend);
                                d.params = ColorParams::lerp(&day, &night, blend);
                                d.active_profile = if blend <= 0.0 {
                                    Some(cfg.day_profile.clone())
                                } else if blend >= 1.0 {
                                    Some(cfg.night_profile.clone())
                                } else {
                                    None
                                };
                                if let Err(e) = d.apply().await {
                                    log::warn!("schedule apply failed: {e:#}");
                                }
                            } else {
                                log::warn!(
                                    "schedule: missing profile `{}` or `{}`",
                                    cfg.day_profile,
                                    cfg.night_profile
                                );
                            }
                        }
                    }
                    None => {
                        d.schedule.mode = "off".into();
                        d.schedule.blend = 0.0;
                        d.schedule.next_transition_secs = None;
                        last_blend = None;
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(tick)).await;
        }
    });
}
