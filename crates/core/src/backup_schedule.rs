//! When backups run by themselves, which of them are kept, and the one function that makes
//! a backup and writes it into the history — for a run asked for in the app and for the
//! runner that the operating system starts while the app is closed ([ADR 0021]).
//!
//! [ADR 0021]: ../../../docs/adr/0021-backups-with-dump-tools.md

use std::sync::atomic::AtomicBool;

use chrono::{Datelike, Duration, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

use crate::backup::{self, BackupRequest};
use crate::db::Session;
use crate::model::ConnectionProfile;
use crate::store::{BackupJob, BackupRun, Store};
use crate::{Error, Result};

/// When a job runs, in the computer's local time.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Schedule {
    /// Only when asked.
    #[default]
    Off,
    Daily {
        hour: u8,
        minute: u8,
    },
    /// `days`: 1 = Monday … 7 = Sunday.
    Weekly {
        days: Vec<u8>,
        hour: u8,
        minute: u8,
    },
}

impl Schedule {
    /// Reject times and days that do not exist; put the days in order, each once.
    pub fn validated(self) -> Result<Self> {
        let time = |hour: u8, minute: u8| {
            if hour > 23 || minute > 59 {
                Err(Error::Invalid(format!(
                    "{hour}:{minute:02} is not a time of day"
                )))
            } else {
                Ok(())
            }
        };
        match self {
            Schedule::Off => Ok(Schedule::Off),
            Schedule::Daily { hour, minute } => {
                time(hour, minute)?;
                Ok(Schedule::Daily { hour, minute })
            }
            Schedule::Weekly {
                mut days,
                hour,
                minute,
            } => {
                time(hour, minute)?;
                days.sort_unstable();
                days.dedup();
                if days.is_empty() || days.iter().any(|d| !(1..=7).contains(d)) {
                    return Err(Error::Invalid(
                        "A weekly schedule needs at least one day of the week".into(),
                    ));
                }
                // Every day of the week is a daily schedule.
                Ok(if days.len() == 7 {
                    Schedule::Daily { hour, minute }
                } else {
                    Schedule::Weekly { days, hour, minute }
                })
            }
        }
    }

    fn time(&self) -> Option<NaiveTime> {
        match self {
            Schedule::Off => None,
            Schedule::Daily { hour, minute } | Schedule::Weekly { hour, minute, .. } => {
                NaiveTime::from_hms_opt(u32::from(*hour), u32::from(*minute), 0)
            }
        }
    }

    fn on(&self, day: chrono::NaiveDate) -> bool {
        match self {
            Schedule::Off => false,
            Schedule::Daily { .. } => true,
            Schedule::Weekly { days, .. } => {
                days.contains(&(day.weekday().number_from_monday() as u8))
            }
        }
    }

    /// The latest scheduled time at or before `now`.
    pub fn previous(&self, now: NaiveDateTime) -> Option<NaiveDateTime> {
        let time = self.time()?;
        (0..=7)
            .map(|back| (now.date() - Duration::days(back)).and_time(time))
            .find(|slot| *slot <= now && self.on(slot.date()))
    }

    /// The first scheduled time after `now`.
    pub fn next(&self, now: NaiveDateTime) -> Option<NaiveDateTime> {
        let time = self.time()?;
        (0..=7)
            .map(|ahead| (now.date() + Duration::days(ahead)).and_time(time))
            .find(|slot| *slot > now && self.on(slot.date()))
    }

    /// The times as launchd's `StartCalendarInterval` wants them: `(weekday, hour, minute)`
    /// with Sunday as 0, and no weekday for every day.
    pub fn calendar(&self) -> Vec<(Option<u8>, u8, u8)> {
        match self {
            Schedule::Off => Vec::new(),
            Schedule::Daily { hour, minute } => vec![(None, *hour, *minute)],
            Schedule::Weekly { days, hour, minute } => {
                days.iter().map(|d| (Some(d % 7), *hour, *minute)).collect()
            }
        }
    }
}

/// Whether a job should run now: a scheduled time has passed that is later than both the
/// job's last scheduled run and the moment its settings were saved. So a job never runs for
/// a time before it existed, and a time missed while the computer slept is made up for once.
pub fn is_due(
    schedule: &Schedule,
    saved: NaiveDateTime,
    last_run: Option<NaiveDateTime>,
    now: NaiveDateTime,
) -> bool {
    let Some(slot) = schedule.previous(now) else {
        return false;
    };
    slot > saved && last_run.is_none_or(|last| slot > last)
}

/// Of a job's scheduled backups that are still on disk, newest first, those beyond the
/// `keep` newest. `keep` 0 keeps all.
pub fn beyond_kept(runs: &[BackupRun], keep: u32) -> Vec<&BackupRun> {
    if keep == 0 {
        return Vec::new();
    }
    runs.iter()
        .filter(|r| r.scheduled && r.status == "ok" && r.file.is_some() && r.removed_at.is_none())
        .skip(keep as usize)
        .collect()
}

fn local(ms: i64) -> NaiveDateTime {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|utc| utc.with_timezone(&chrono::Local).naive_local())
        .unwrap_or_default()
}

/// What a backup is made with and for whom.
pub struct Subject<'a> {
    pub profile: &'a ConnectionProfile,
    pub password: Option<&'a str>,
    /// As the server reported it; the dump tool is chosen for it.
    pub server_version: &'a str,
    pub scheduled: bool,
    /// Scheduled backups of this database to keep; 0 keeps all.
    pub keep: u32,
    /// When the run counts as started (milliseconds since the epoch).
    pub started_at: i64,
}

/// Make one backup and write it into the history, whatever the outcome: kept, failed or
/// stopped. After a scheduled backup that was kept, the older ones beyond `keep` are
/// deleted. An error is returned only when the history itself cannot be written.
pub async fn perform(
    store: &Store,
    subject: Subject<'_>,
    request: &BackupRequest,
    stop: &AtomicBool,
    on_progress: impl FnMut(u64),
) -> Result<BackupRun> {
    let started_at = subject.started_at;
    let clock = std::time::Instant::now();
    let tools = backup::find_tools(subject.profile.engine).await;
    let choice = backup::choose(subject.profile.engine, subject.server_version, &tools);
    let mut run = BackupRun {
        id: 0,
        started_at,
        finished_at: started_at,
        connection_id: subject.profile.id.clone(),
        connection_name: subject.profile.name.clone(),
        database: request.database.clone(),
        scheduled: subject.scheduled,
        status: "failed".into(),
        file: None,
        bytes: None,
        tool: String::new(),
        message: String::new(),
        removed_at: None,
    };
    match choice.tool {
        None => run.message = choice.problem.unwrap_or_default(),
        Some(tool) => {
            let name = std::path::Path::new(&tool.path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            run.tool = format!("{name} {}", tool.version);
            match backup::run(
                subject.profile,
                subject.password,
                &tool,
                request,
                stop,
                on_progress,
            )
            .await
            {
                Ok(done) => {
                    run.status = "ok".into();
                    run.file = Some(done.file.to_string_lossy().into_owned());
                    run.bytes = Some(done.bytes as i64);
                }
                Err(Error::Conflict(message)) if message.contains("stopped") => {
                    run.status = "stopped".into();
                    run.message = message;
                }
                Err(e) => run.message = e.to_string(),
            }
        }
    }
    run.finished_at = started_at + clock.elapsed().as_millis() as i64;
    run.id = store.add_backup_run(&run).await?;

    if run.status == "ok" && subject.scheduled {
        let history = store
            .list_backup_runs(Some((&run.connection_id, &run.database)), 1000)
            .await?;
        for old in beyond_kept(&history, subject.keep) {
            // Only a file this history recorded, and only now that a newer one exists.
            if let Some(file) = &old.file {
                match std::fs::remove_file(file) {
                    Ok(()) => store.mark_backup_removed(old.id).await?,
                    // Moved or deleted by the user: it is gone either way.
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        store.mark_backup_removed(old.id).await?
                    }
                    Err(_) => {}
                }
            }
        }
    }
    Ok(run)
}

/// Run every job whose scheduled time has come. `password_of` reads a connection's password
/// from the credential store. The runs are returned, newest last; each is in the history.
pub async fn run_due(
    store: &Store,
    password_of: impl Fn(&ConnectionProfile) -> Result<Option<String>>,
    now_ms: i64,
) -> Result<Vec<BackupRun>> {
    let now = local(now_ms);
    let mut done = Vec::new();
    for job in store.list_backup_jobs().await? {
        if job.schedule == Schedule::Off {
            continue;
        }
        let last = store
            .list_backup_runs(Some((&job.connection_id, &job.database)), 200)
            .await?
            .into_iter()
            .find(|r| r.scheduled)
            .map(|r| local(r.started_at));
        if !is_due(&job.schedule, local(job.updated_at), last, now) {
            continue;
        }
        done.push(run_job(store, &job, &password_of, now_ms).await?);
    }
    Ok(done)
}

/// One scheduled run of a job, from reading the password to the entry in the history.
async fn run_job(
    store: &Store,
    job: &BackupJob,
    password_of: &impl Fn(&ConnectionProfile) -> Result<Option<String>>,
    started_at: i64,
) -> Result<BackupRun> {
    let request = BackupRequest {
        database: job.database.clone(),
        destination: job.destination.clone().into(),
        format: job.format,
        compress: job.compress,
    };
    // Whatever goes wrong before the dump tool starts is a failed run too, with its reason.
    let failed = |profile: Option<&ConnectionProfile>, message: String| BackupRun {
        id: 0,
        started_at,
        finished_at: started_at,
        connection_id: job.connection_id.clone(),
        connection_name: profile.map(|p| p.name.clone()).unwrap_or_default(),
        database: job.database.clone(),
        scheduled: true,
        status: "failed".into(),
        file: None,
        bytes: None,
        tool: String::new(),
        message,
        removed_at: None,
    };
    let record = |mut run: BackupRun| async move {
        run.id = store.add_backup_run(&run).await?;
        Ok(run)
    };
    let profile = match store.get_connection(&job.connection_id).await {
        Ok(profile) => profile,
        Err(e) => {
            return record(failed(
                None,
                format!("The connection of this backup no longer exists ({e})."),
            ))
            .await
        }
    };
    let password = match password_of(&profile) {
        Ok(password) => password,
        Err(e) => {
            return record(failed(
                Some(&profile),
                format!("The password could not be read from the credential store: {e}"),
            ))
            .await
        }
    };
    let version = match Session::connect(&profile, password.as_deref()).await {
        Ok(session) => {
            let info = session.server_info().await;
            session.close().await;
            match info {
                Ok(info) => info.version,
                Err(e) => {
                    return record(failed(
                        Some(&profile),
                        format!("The server did not answer: {e}"),
                    ))
                    .await
                }
            }
        }
        Err(e) => return record(failed(Some(&profile), format!("Could not connect: {e}"))).await,
    };
    perform(
        store,
        Subject {
            profile: &profile,
            password: password.as_deref(),
            server_version: &version,
            scheduled: true,
            keep: job.keep,
            started_at,
        },
        &request,
        &AtomicBool::new(false),
        |_| {},
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        // November 2026: the 16th is a Monday.
        NaiveDate::from_ymd_opt(2026, 11, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
    }

    fn run(id: i64, scheduled: bool, status: &str, removed: bool) -> BackupRun {
        BackupRun {
            id,
            started_at: id,
            finished_at: id,
            connection_id: "c".into(),
            connection_name: "C".into(),
            database: "d".into(),
            scheduled,
            status: status.into(),
            file: (status == "ok").then(|| format!("/b/{id}.sql.gz")),
            bytes: None,
            tool: String::new(),
            message: String::new(),
            removed_at: removed.then_some(1),
        }
    }

    #[test]
    fn schedules_are_checked_and_tidied() {
        assert_eq!(
            Schedule::Daily {
                hour: 2,
                minute: 30
            }
            .validated()
            .unwrap(),
            Schedule::Daily {
                hour: 2,
                minute: 30
            }
        );
        assert!(Schedule::Daily {
            hour: 24,
            minute: 0
        }
        .validated()
        .is_err());
        assert!(Schedule::Weekly {
            days: vec![1],
            hour: 2,
            minute: 60
        }
        .validated()
        .is_err());
        assert!(Schedule::Weekly {
            days: vec![],
            hour: 2,
            minute: 0
        }
        .validated()
        .is_err());
        assert!(Schedule::Weekly {
            days: vec![0],
            hour: 2,
            minute: 0
        }
        .validated()
        .is_err());
        assert_eq!(
            Schedule::Weekly {
                days: vec![5, 1, 5],
                hour: 2,
                minute: 0
            }
            .validated()
            .unwrap(),
            Schedule::Weekly {
                days: vec![1, 5],
                hour: 2,
                minute: 0
            }
        );
        assert_eq!(
            Schedule::Weekly {
                days: vec![7, 6, 5, 4, 3, 2, 1],
                hour: 2,
                minute: 0
            }
            .validated()
            .unwrap(),
            Schedule::Daily { hour: 2, minute: 0 }
        );
        // The stored form.
        assert_eq!(
            serde_json::to_string(&Schedule::Off).unwrap(),
            r#"{"kind":"off"}"#
        );
        assert_eq!(
            serde_json::from_str::<Schedule>(
                r#"{"kind":"weekly","days":[1,3],"hour":2,"minute":0}"#
            )
            .unwrap(),
            Schedule::Weekly {
                days: vec![1, 3],
                hour: 2,
                minute: 0
            }
        );
    }

    #[test]
    fn the_previous_and_the_next_time() {
        let daily = Schedule::Daily { hour: 2, minute: 0 };
        assert_eq!(daily.previous(at(19, 1, 59)), Some(at(18, 2, 0)));
        assert_eq!(daily.previous(at(19, 2, 0)), Some(at(19, 2, 0)));
        assert_eq!(daily.next(at(19, 2, 0)), Some(at(20, 2, 0)));
        assert_eq!(daily.next(at(19, 1, 59)), Some(at(19, 2, 0)));

        // Monday and Friday at 23:30. The 19th is a Thursday.
        let weekly = Schedule::Weekly {
            days: vec![1, 5],
            hour: 23,
            minute: 30,
        };
        assert_eq!(weekly.previous(at(19, 12, 0)), Some(at(16, 23, 30)));
        assert_eq!(weekly.next(at(19, 12, 0)), Some(at(20, 23, 30)));
        assert_eq!(weekly.next(at(20, 23, 30)), Some(at(23, 23, 30)));
        // The same weekday, a week apart.
        let monday = Schedule::Weekly {
            days: vec![1],
            hour: 9,
            minute: 0,
        };
        assert_eq!(monday.previous(at(16, 8, 0)), Some(at(9, 9, 0)));
        assert_eq!(monday.next(at(16, 9, 0)), Some(at(23, 9, 0)));

        assert_eq!(Schedule::Off.previous(at(19, 12, 0)), None);
        assert_eq!(Schedule::Off.next(at(19, 12, 0)), None);
    }

    #[test]
    fn launchd_counts_sunday_as_zero() {
        assert_eq!(
            Schedule::Daily { hour: 2, minute: 5 }.calendar(),
            [(None, 2, 5)]
        );
        assert_eq!(
            Schedule::Weekly {
                days: vec![1, 7],
                hour: 2,
                minute: 5
            }
            .calendar(),
            [(Some(1), 2, 5), (Some(0), 2, 5)]
        );
        assert!(Schedule::Off.calendar().is_empty());
    }

    #[test]
    fn a_job_is_due_once_per_scheduled_time_and_never_for_a_time_before_it_was_saved() {
        let daily = Schedule::Daily { hour: 2, minute: 0 };
        // Saved in the afternoon: this morning's time is not made up for.
        assert!(!is_due(&daily, at(18, 15, 0), None, at(18, 16, 0)));
        // The next morning it is due, once.
        assert!(is_due(&daily, at(18, 15, 0), None, at(19, 2, 0)));
        assert!(!is_due(
            &daily,
            at(18, 15, 0),
            Some(at(19, 2, 0)),
            at(19, 2, 5)
        ));
        assert!(!is_due(
            &daily,
            at(18, 15, 0),
            Some(at(19, 2, 0)),
            at(20, 1, 59)
        ));
        // The computer slept for three nights: one run when it wakes, not three.
        assert!(is_due(
            &daily,
            at(15, 15, 0),
            Some(at(16, 2, 0)),
            at(19, 9, 0)
        ));
        assert!(!is_due(
            &daily,
            at(15, 15, 0),
            Some(at(19, 9, 0)),
            at(19, 9, 1)
        ));
        // Changing the settings counts as saving: the time that just passed is not run.
        assert!(!is_due(
            &daily,
            at(19, 2, 30),
            Some(at(18, 2, 0)),
            at(19, 3, 0)
        ));
        assert!(!is_due(&Schedule::Off, at(1, 0, 0), None, at(19, 9, 0)));
    }

    #[test]
    fn only_scheduled_backups_that_are_still_there_count_for_retention() {
        // Newest first.
        let runs = [
            run(9, true, "ok", false),
            run(8, false, "ok", false), // made by hand: never deleted, never counted
            run(7, true, "failed", false),
            run(6, true, "ok", false),
            run(5, true, "ok", true), // already removed
            run(4, true, "ok", false),
            run(3, true, "ok", false),
        ];
        let ids = |keep| {
            beyond_kept(&runs, keep)
                .iter()
                .map(|r| r.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(2), [4, 3]);
        assert_eq!(ids(4), Vec::<i64>::new());
        assert_eq!(ids(1), [6, 4, 3]);
        assert_eq!(ids(0), Vec::<i64>::new());
    }
}
