use anyhow::{bail, ensure, Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwitchSource {
    CtrlEdge,
    EdgePicker,
    Hotkey,
    Menu,
    DisconnectGuard,
    ClamshellGuard,
    SleepGuard,
}

impl SwitchSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CtrlEdge => "ctrl_edge",
            Self::EdgePicker => "edge_picker",
            Self::Hotkey => "hotkey",
            Self::Menu => "menu",
            Self::DisconnectGuard => "disconnect_guard",
            Self::ClamshellGuard => "clamshell_guard",
            Self::SleepGuard => "sleep_guard",
        }
    }
}

impl FromStr for SwitchSource {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "ctrl_edge" => Ok(Self::CtrlEdge),
            "edge_picker" => Ok(Self::EdgePicker),
            "hotkey" => Ok(Self::Hotkey),
            "menu" => Ok(Self::Menu),
            "disconnect_guard" => Ok(Self::DisconnectGuard),
            "clamshell_guard" => Ok(Self::ClamshellGuard),
            "sleep_guard" => Ok(Self::SleepGuard),
            other => bail!("未知切屏触发源: {other}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwitchStatus {
    Pending,
    Writing,
    Settling,
    Verifying,
    Done,
    Failed,
    Superseded,
}

impl SwitchStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Writing => "writing",
            Self::Settling => "settling",
            Self::Verifying => "verifying",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Superseded => "superseded",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Superseded)
    }
}

impl FromStr for SwitchStatus {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "pending" => Ok(Self::Pending),
            "writing" => Ok(Self::Writing),
            "settling" => Ok(Self::Settling),
            "verifying" => Ok(Self::Verifying),
            "done" => Ok(Self::Done),
            "failed" => Ok(Self::Failed),
            "superseded" => Ok(Self::Superseded),
            other => bail!("未知切屏状态: {other}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SwitchRequestRow {
    pub id: i64,
    pub request_key: String,
    pub source: SwitchSource,
    pub controller: String,
    pub from_host: Option<String>,
    pub to_host: Option<String>,
    pub target_input: u8,
    pub observed_input: Option<u8>,
    pub status: SwitchStatus,
    pub attempts: u8,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControllerRuntimeRow {
    pub controller: String,
    pub last_ddc_write_at: Option<DateTime<Utc>>,
    pub preferred_brightness: u8,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewSwitchRequest {
    pub request_key: String,
    pub source: SwitchSource,
    pub controller: String,
    pub from_host: Option<String>,
    pub to_host: Option<String>,
    pub target_input: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueOutcome {
    DuplicateIgnored(SwitchRequestRow),
    ReadyToWrite(SwitchRequestRow),
    QueuedPending {
        row: SwitchRequestRow,
        superseded_id: Option<i64>,
        wait_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    Done(SwitchRequestRow),
    RetryWrite(SwitchRequestRow),
    Failed(SwitchRequestRow),
    SupersededByPending(SwitchRequestRow),
}

pub struct StateDb {
    path: PathBuf,
}

impl StateDb {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Self {
            path: path.to_path_buf(),
        };
        let conn = db.connect()?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS controller_runtime (
                controller TEXT PRIMARY KEY,
                last_ddc_write_at TEXT,
                preferred_brightness INTEGER NOT NULL DEFAULT 85 CHECK (preferred_brightness BETWEEN 0 AND 100),
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS switch_requests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                request_key TEXT NOT NULL UNIQUE,
                source TEXT NOT NULL CHECK (source IN ('ctrl_edge','edge_picker','hotkey','menu','disconnect_guard','clamshell_guard','sleep_guard')),
                controller TEXT NOT NULL,
                from_host TEXT,
                to_host TEXT,
                target_input INTEGER NOT NULL CHECK (target_input BETWEEN 1 AND 255),
                observed_input INTEGER,
                status TEXT NOT NULL CHECK (status IN ('pending','writing','settling','verifying','done','failed','superseded')),
                attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 2),
                error TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                finished_at TEXT
            );

            CREATE UNIQUE INDEX IF NOT EXISTS one_executing
                ON switch_requests(controller)
                WHERE status IN ('writing','settling','verifying');

            CREATE UNIQUE INDEX IF NOT EXISTS one_pending
                ON switch_requests(controller)
                WHERE status = 'pending';

            CREATE INDEX IF NOT EXISTS idx_switch_recent
                ON switch_requests(created_at DESC);

            CREATE INDEX IF NOT EXISTS idx_switch_failed
                ON switch_requests(created_at DESC)
                WHERE status = 'failed';
            "#,
        )
        .context("初始化 SQLite 表结构失败")?;
        Ok(db)
    }

    fn connect(&self) -> Result<Connection> {
        let conn = Connection::open(&self.path)
            .with_context(|| format!("打开状态库失败: {}", self.path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 3000; PRAGMA foreign_keys = ON;",
        )?;
        Ok(conn)
    }

    pub fn ensure_runtime(
        &self,
        controller: &str,
        preferred_brightness: u8,
        now: DateTime<Utc>,
    ) -> Result<ControllerRuntimeRow> {
        ensure!(preferred_brightness <= 100, "亮度必须在 0..=100 之间");
        let conn = self.connect()?;
        let now_str = now.to_rfc3339();
        conn.execute(
            r#"
            INSERT INTO controller_runtime (controller, last_ddc_write_at, preferred_brightness, updated_at)
            VALUES (?1, NULL, ?2, ?3)
            ON CONFLICT(controller) DO NOTHING
            "#,
            params![controller, i64::from(preferred_brightness), now_str],
        )?;
        Self::get_runtime_conn(&conn, controller)
    }

    pub fn set_preferred_brightness(
        &self,
        controller: &str,
        brightness: u8,
        now: DateTime<Utc>,
    ) -> Result<()> {
        ensure!(brightness <= 100, "亮度必须在 0..=100 之间");
        let conn = self.connect()?;
        let now_str = now.to_rfc3339();
        conn.execute(
            r#"
            INSERT INTO controller_runtime (controller, last_ddc_write_at, preferred_brightness, updated_at)
            VALUES (?1, NULL, ?2, ?3)
            ON CONFLICT(controller) DO UPDATE SET
                preferred_brightness = excluded.preferred_brightness,
                updated_at = excluded.updated_at
            "#,
            params![controller, i64::from(brightness), now_str],
        )?;
        Ok(())
    }

    pub fn get_runtime(&self, controller: &str) -> Result<Option<ControllerRuntimeRow>> {
        let conn = self.connect()?;
        Self::get_runtime_opt_conn(&conn, controller)
    }

    pub fn recover_after_restart(&self, controller: &str, now: DateTime<Utc>) -> Result<usize> {
        let conn = self.connect()?;
        let now_str = now.to_rfc3339();
        let affected = conn.execute(
            r#"
            UPDATE switch_requests
            SET status = 'failed',
                error = COALESCE(error, '进程重启恢复：中断未完成切换'),
                updated_at = ?1,
                finished_at = ?1
            WHERE controller = ?2
              AND status IN ('writing', 'settling', 'verifying')
            "#,
            params![now_str, controller],
        )?;
        Ok(affected)
    }

    pub fn enqueue_switch(
        &self,
        req: NewSwitchRequest,
        settle_ms: u64,
        now: DateTime<Utc>,
    ) -> Result<EnqueueOutcome> {
        ensure!(req.target_input >= 1, "target_input 必须在 1..=255 之间");
        ensure!(!req.request_key.trim().is_empty(), "request_key 不能为空");

        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now_str = now.to_rfc3339();

        tx.execute(
            r#"
            INSERT INTO controller_runtime (controller, last_ddc_write_at, preferred_brightness, updated_at)
            VALUES (?1, NULL, 85, ?2)
            ON CONFLICT(controller) DO NOTHING
            "#,
            params![req.controller, now_str],
        )?;

        if let Some(existing) = Self::get_by_key_conn(&tx, &req.request_key)? {
            tx.commit()?;
            return Ok(EnqueueOutcome::DuplicateIgnored(existing));
        }

        let executing_exists: Option<i64> = tx
            .query_row(
                "SELECT id FROM switch_requests WHERE controller = ?1 AND status IN ('writing','settling','verifying') LIMIT 1",
                params![req.controller],
                |r| r.get(0),
            )
            .optional()?;

        let rt = Self::get_runtime_conn(&tx, &req.controller)?;
        let cooldown_remaining_ms = Self::remaining_cooldown_ms(rt.last_ddc_write_at, settle_ms, now);

        if executing_exists.is_none() && cooldown_remaining_ms == 0 {
            tx.execute(
                r#"
                INSERT INTO switch_requests (
                    request_key, source, controller, from_host, to_host,
                    target_input, status, attempts, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'writing', 0, ?7, ?7)
                "#,
                params![
                    req.request_key,
                    req.source.as_str(),
                    req.controller,
                    req.from_host,
                    req.to_host,
                    i64::from(req.target_input),
                    now_str
                ],
            )?;
            let id = tx.last_insert_rowid();
            let row = Self::get_by_id_conn(&tx, id)?;
            tx.commit()?;
            return Ok(EnqueueOutcome::ReadyToWrite(row));
        }

        let old_pending: Option<i64> = tx
            .query_row(
                "SELECT id FROM switch_requests WHERE controller = ?1 AND status = 'pending' LIMIT 1",
                params![req.controller],
                |r| r.get(0),
            )
            .optional()?;

        if let Some(old_id) = old_pending {
            tx.execute(
                r#"
                UPDATE switch_requests
                SET status = 'superseded',
                    updated_at = ?1,
                    finished_at = ?1
                WHERE id = ?2
                "#,
                params![now_str, old_id],
            )?;
        }

        tx.execute(
            r#"
            INSERT INTO switch_requests (
                request_key, source, controller, from_host, to_host,
                target_input, status, attempts, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', 0, ?7, ?7)
            "#,
            params![
                req.request_key,
                req.source.as_str(),
                req.controller,
                req.from_host,
                req.to_host,
                i64::from(req.target_input),
                now_str
            ],
        )?;
        let id = tx.last_insert_rowid();
        let row = Self::get_by_id_conn(&tx, id)?;
        tx.commit()?;

        let wait_ms = if cooldown_remaining_ms > 0 {
            cooldown_remaining_ms
        } else {
            200
        };
        Ok(EnqueueOutcome::QueuedPending {
            row,
            superseded_id: old_pending,
            wait_ms,
        })
    }

    pub fn promote_pending_if_ready(
        &self,
        controller: &str,
        settle_ms: u64,
        now: DateTime<Utc>,
    ) -> Result<Option<SwitchRequestRow>> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        let executing_exists: Option<i64> = tx
            .query_row(
                "SELECT id FROM switch_requests WHERE controller = ?1 AND status IN ('writing','settling','verifying') LIMIT 1",
                params![controller],
                |r| r.get(0),
            )
            .optional()?;
        if executing_exists.is_some() {
            return Ok(None);
        }

        let Some(rt) = Self::get_runtime_opt_conn(&tx, controller)? else {
            return Ok(None);
        };
        if Self::remaining_cooldown_ms(rt.last_ddc_write_at, settle_ms, now) > 0 {
            return Ok(None);
        }

        let pending_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM switch_requests WHERE controller = ?1 AND status = 'pending' LIMIT 1",
                params![controller],
                |r| r.get(0),
            )
            .optional()?;
        let Some(id) = pending_id else {
            return Ok(None);
        };

        let now_str = now.to_rfc3339();
        tx.execute(
            "UPDATE switch_requests SET status = 'writing', updated_at = ?1 WHERE id = ?2",
            params![now_str, id],
        )?;
        let row = Self::get_by_id_conn(&tx, id)?;
        tx.commit()?;
        Ok(Some(row))
    }

    pub fn record_ddc_write_attempt(
        &self,
        id: i64,
        controller: &str,
        write_error: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<SwitchRequestRow> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let cur = Self::get_by_id_conn(&tx, id)?;
        ensure!(
            cur.status == SwitchStatus::Writing,
            "仅 writing 状态可记录 DDC 写入（当前为 {}）",
            cur.status.as_str()
        );
        ensure!(cur.attempts < 2, "写入次数已达上限 2 次");

        let next_attempts = cur.attempts + 1;
        let now_str = now.to_rfc3339();

        tx.execute(
            r#"
            INSERT INTO controller_runtime (controller, last_ddc_write_at, preferred_brightness, updated_at)
            VALUES (?1, ?2, 85, ?2)
            ON CONFLICT(controller) DO UPDATE SET
                last_ddc_write_at = excluded.last_ddc_write_at,
                updated_at = excluded.updated_at
            "#,
            params![controller, now_str],
        )?;

        if let Some(err_msg) = write_error {
            if next_attempts >= 2 {
                tx.execute(
                    r#"
                    UPDATE switch_requests
                    SET status = 'failed',
                        attempts = ?1,
                        error = ?2,
                        updated_at = ?3,
                        finished_at = ?3
                    WHERE id = ?4
                    "#,
                    params![i64::from(next_attempts), err_msg, now_str, id],
                )?;
                let row = Self::get_by_id_conn(&tx, id)?;
                tx.commit()?;
                return Ok(row);
            }
        }

        tx.execute(
            r#"
            UPDATE switch_requests
            SET status = 'settling',
                attempts = ?1,
                error = ?2,
                updated_at = ?3
            WHERE id = ?4
            "#,
            params![i64::from(next_attempts), write_error, now_str, id],
        )?;
        let row = Self::get_by_id_conn(&tx, id)?;
        tx.commit()?;
        Ok(row)
    }

    pub fn begin_verifying_if_settled(
        &self,
        controller: &str,
        settle_ms: u64,
        now: DateTime<Utc>,
    ) -> Result<Option<SwitchRequestRow>> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        let settling_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM switch_requests WHERE controller = ?1 AND status = 'settling' LIMIT 1",
                params![controller],
                |r| r.get(0),
            )
            .optional()?;
        let Some(id) = settling_id else {
            return Ok(None);
        };

        let rt = Self::get_runtime_conn(&tx, controller)?;
        if Self::remaining_cooldown_ms(rt.last_ddc_write_at, settle_ms, now) > 0 {
            return Ok(None);
        }

        let now_str = now.to_rfc3339();
        tx.execute(
            "UPDATE switch_requests SET status = 'verifying', updated_at = ?1 WHERE id = ?2",
            params![now_str, id],
        )?;
        let row = Self::get_by_id_conn(&tx, id)?;
        tx.commit()?;
        Ok(Some(row))
    }

    pub fn complete_verification(
        &self,
        id: i64,
        controller: &str,
        observed_input: Option<u8>,
        read_error: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<VerifyOutcome> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let cur = Self::get_by_id_conn(&tx, id)?;
        ensure!(
            cur.status == SwitchStatus::Verifying,
            "仅 verifying 状态可完成回读校验（当前为 {}）",
            cur.status.as_str()
        );

        let now_str = now.to_rfc3339();
        let obs_db = observed_input.map(i64::from);

        let has_pending: Option<i64> = tx
            .query_row(
                "SELECT id FROM switch_requests WHERE controller = ?1 AND status = 'pending' LIMIT 1",
                params![controller],
                |r| r.get(0),
            )
            .optional()?;

        if has_pending.is_some() {
            tx.execute(
                r#"
                UPDATE switch_requests
                SET status = 'superseded',
                    observed_input = ?1,
                    updated_at = ?2,
                    finished_at = ?2
                WHERE id = ?3
                "#,
                params![obs_db, now_str, id],
            )?;
            let row = Self::get_by_id_conn(&tx, id)?;
            tx.commit()?;
            return Ok(VerifyOutcome::SupersededByPending(row));
        }

        let matched = read_error.is_none()
            && observed_input.is_some_and(|v| v != 0 && v == cur.target_input);

        if matched {
            tx.execute(
                r#"
                UPDATE switch_requests
                SET status = 'done',
                    observed_input = ?1,
                    error = NULL,
                    updated_at = ?2,
                    finished_at = ?2
                WHERE id = ?3
                "#,
                params![obs_db, now_str, id],
            )?;
            let row = Self::get_by_id_conn(&tx, id)?;
            tx.commit()?;
            return Ok(VerifyOutcome::Done(row));
        }

        let reason = match (read_error, observed_input) {
            (Some(e), _) => format!("回读失败: {e}"),
            (None, Some(0)) => "回读到非法输入源 0".to_string(),
            (None, Some(v)) => format!("回读输入源 ({v}) 与目标 ({}) 不一致", cur.target_input),
            (None, None) => "回读无返回值".to_string(),
        };

        if cur.attempts < 2 {
            tx.execute(
                r#"
                UPDATE switch_requests
                SET status = 'writing',
                    observed_input = ?1,
                    error = ?2,
                    updated_at = ?3
                WHERE id = ?4
                "#,
                params![obs_db, reason, now_str, id],
            )?;
            let row = Self::get_by_id_conn(&tx, id)?;
            tx.commit()?;
            return Ok(VerifyOutcome::RetryWrite(row));
        }

        tx.execute(
            r#"
            UPDATE switch_requests
            SET status = 'failed',
                observed_input = ?1,
                error = ?2,
                updated_at = ?3,
                finished_at = ?3
            WHERE id = ?4
            "#,
            params![obs_db, reason, now_str, id],
        )?;
        let row = Self::get_by_id_conn(&tx, id)?;
        tx.commit()?;
        Ok(VerifyOutcome::Failed(row))
    }

    pub fn active_executing_request(&self, controller: &str) -> Result<Option<SwitchRequestRow>> {
        let conn = self.connect()?;
        let id: Option<i64> = conn
            .query_row(
                "SELECT id FROM switch_requests WHERE controller = ?1 AND status IN ('writing','settling','verifying') LIMIT 1",
                params![controller],
                |r| r.get(0),
            )
            .optional()?;
        match id {
            Some(i) => Ok(Some(Self::get_by_id_conn(&conn, i)?)),
            None => Ok(None),
        }
    }

    pub fn has_pending_request(&self, controller: &str) -> Result<bool> {
        let conn = self.connect()?;
        let id: Option<i64> = conn
            .query_row(
                "SELECT id FROM switch_requests WHERE controller = ?1 AND status = 'pending' LIMIT 1",
                params![controller],
                |r| r.get(0),
            )
            .optional()?;
        Ok(id.is_some())
    }

    pub fn list_recent_requests(&self, limit: usize) -> Result<Vec<SwitchRequestRow>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, request_key, source, controller, from_host, to_host,
                   target_input, observed_input, status, attempts, error,
                   created_at, updated_at, finished_at
            FROM switch_requests
            ORDER BY id DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt.query_map(params![limit as i64], Self::map_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn prune_older_than_days(&self, days: i64, now: DateTime<Utc>) -> Result<usize> {
        let cutoff = (now - Duration::days(days)).to_rfc3339();
        let conn = self.connect()?;
        let deleted = conn.execute(
            r#"
            DELETE FROM switch_requests
            WHERE status IN ('done', 'failed', 'superseded')
              AND created_at < ?1
            "#,
            params![cutoff],
        )?;
        Ok(deleted)
    }

    pub fn remaining_cooldown_ms(
        last_ddc_write_at: Option<DateTime<Utc>>,
        settle_ms: u64,
        now: DateTime<Utc>,
    ) -> u64 {
        let Some(last) = last_ddc_write_at else {
            return 0;
        };
        let elapsed_ms = (now - last).num_milliseconds();
        if elapsed_ms < 0 {
            return settle_ms;
        }
        let elapsed_u64 = elapsed_ms as u64;
        settle_ms.saturating_sub(elapsed_u64)
    }

    fn get_runtime_conn(conn: &Connection, controller: &str) -> Result<ControllerRuntimeRow> {
        Self::get_runtime_opt_conn(conn, controller)?.context("未找到 controller_runtime 记录")
    }

    fn get_runtime_opt_conn(
        conn: &Connection,
        controller: &str,
    ) -> Result<Option<ControllerRuntimeRow>> {
        let row = conn
            .query_row(
                "SELECT controller, last_ddc_write_at, preferred_brightness, updated_at FROM controller_runtime WHERE controller = ?1",
                params![controller],
                |r| {
                    let ctrl: String = r.get(0)?;
                    let last_str: Option<String> = r.get(1)?;
                    let brightness: i64 = r.get(2)?;
                    let updated_str: String = r.get(3)?;
                    Ok((ctrl, last_str, brightness, updated_str))
                },
            )
            .optional()?;

        let Some((ctrl, last_str, brightness, updated_str)) = row else {
            return Ok(None);
        };
        let last_ddc_write_at = match last_str {
            Some(s) => Some(DateTime::parse_from_rfc3339(&s)?.with_timezone(&Utc)),
            None => None,
        };
        let updated_at = DateTime::parse_from_rfc3339(&updated_str)?.with_timezone(&Utc);
        Ok(Some(ControllerRuntimeRow {
            controller: ctrl,
            last_ddc_write_at,
            preferred_brightness: brightness as u8,
            updated_at,
        }))
    }

    fn get_by_key_conn(conn: &Connection, key: &str) -> Result<Option<SwitchRequestRow>> {
        conn.query_row(
            r#"
            SELECT id, request_key, source, controller, from_host, to_host,
                   target_input, observed_input, status, attempts, error,
                   created_at, updated_at, finished_at
            FROM switch_requests
            WHERE request_key = ?1
            "#,
            params![key],
            Self::map_row,
        )
        .optional()
        .map_err(Into::into)
    }

    fn get_by_id_conn(conn: &Connection, id: i64) -> Result<SwitchRequestRow> {
        conn.query_row(
            r#"
            SELECT id, request_key, source, controller, from_host, to_host,
                   target_input, observed_input, status, attempts, error,
                   created_at, updated_at, finished_at
            FROM switch_requests
            WHERE id = ?1
            "#,
            params![id],
            Self::map_row,
        )
        .map_err(Into::into)
    }

    fn map_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<SwitchRequestRow> {
        let source_str: String = r.get(2)?;
        let status_str: String = r.get(8)?;
        let target_input: i64 = r.get(6)?;
        let observed_input: Option<i64> = r.get(7)?;
        let attempts: i64 = r.get(9)?;
        let created_str: String = r.get(11)?;
        let updated_str: String = r.get(12)?;
        let finished_str: Option<String> = r.get(13)?;

        let parse_ts = |s: &str| -> rusqlite::Result<DateTime<Utc>> {
            DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(11, rusqlite::types::Type::Text, Box::new(e)))
        };

        let source = SwitchSource::from_str(&source_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, e.into())
        })?;
        let status = SwitchStatus::from_str(&status_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, e.into())
        })?;

        Ok(SwitchRequestRow {
            id: r.get(0)?,
            request_key: r.get(1)?,
            source,
            controller: r.get(3)?,
            from_host: r.get(4)?,
            to_host: r.get(5)?,
            target_input: target_input as u8,
            observed_input: observed_input.map(|v| v as u8),
            status,
            attempts: attempts as u8,
            error: r.get(10)?,
            created_at: parse_ts(&created_str)?,
            updated_at: parse_ts(&updated_str)?,
            finished_at: match finished_str {
                Some(s) => Some(parse_ts(&s)?),
                None => None,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_req(key: &str, to_host: &str, input: u8) -> NewSwitchRequest {
        NewSwitchRequest {
            request_key: key.to_string(),
            source: SwitchSource::CtrlEdge,
            controller: "macbook".to_string(),
            from_host: Some("win-desktop".to_string()),
            to_host: Some(to_host.to_string()),
            target_input: input,
        }
    }

    #[test]
    fn idempotent_request_key_deduplicates_exact_row() {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).unwrap();
        let t0 = Utc::now();

        let first = db.enqueue_switch(sample_req("req-1", "macbook", 21), 5000, t0).unwrap();
        let EnqueueOutcome::ReadyToWrite(r1) = first else {
            panic!("expected ReadyToWrite");
        };
        assert_eq!(r1.id, 1);
        assert_eq!(r1.status, SwitchStatus::Writing);
        assert_eq!(r1.target_input, 21);

        let dup = db
            .enqueue_switch(sample_req("req-1", "mac-studio", 15), 5000, t0 + Duration::milliseconds(50))
            .unwrap();
        assert_eq!(dup, EnqueueOutcome::DuplicateIgnored(r1));
        assert_eq!(db.list_recent_requests(10).unwrap().len(), 1);
    }

    #[test]
    fn rapid_burst_during_writing_supersedes_pending_without_conflict_and_keeps_cooldown() {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).unwrap();
        let t0 = Utc::now();

        // 1st request enters writing and writes DDC at t0 + 100ms
        let EnqueueOutcome::ReadyToWrite(r1) = db
            .enqueue_switch(sample_req("k1", "macbook", 21), 5000, t0)
            .unwrap()
        else {
            panic!("expected ReadyToWrite");
        };
        let t_write = t0 + Duration::milliseconds(100);
        let r1_settling = db.record_ddc_write_attempt(r1.id, "macbook", None, t_write).unwrap();
        assert_eq!(r1_settling.status, SwitchStatus::Settling);
        assert_eq!(r1_settling.attempts, 1);

        // 2nd request arrives 500ms later -> queued pending
        let out2 = db
            .enqueue_switch(sample_req("k2", "win-desktop", 17), 5000, t_write + Duration::milliseconds(500))
            .unwrap();
        let EnqueueOutcome::QueuedPending { row: r2, superseded_id, wait_ms } = out2 else {
            panic!("expected QueuedPending");
        };
        assert_eq!(superseded_id, None);
        assert_eq!(r2.status, SwitchStatus::Pending);
        assert_eq!(wait_ms, 4500);

        // 3rd request arrives 1200ms after write -> atomically supersedes r2 without unique conflict
        let out3 = db
            .enqueue_switch(sample_req("k3", "mac-studio", 15), 5000, t_write + Duration::milliseconds(1200))
            .unwrap();
        let EnqueueOutcome::QueuedPending { row: r3, superseded_id: sup3, wait_ms: wait3 } = out3 else {
            panic!("expected QueuedPending");
        };
        assert_eq!(sup3, Some(r2.id));
        assert_eq!(r3.target_input, 15);
        assert_eq!(wait3, 3800);

        // Cooldown anchor must remain exact t_write
        let rt = db.get_runtime("macbook").unwrap().unwrap();
        assert_eq!(rt.last_ddc_write_at, Some(t_write));

        // Cannot promote before settle_ms expires
        assert!(db
            .promote_pending_if_ready("macbook", 5000, t_write + Duration::milliseconds(4999))
            .unwrap()
            .is_none());
    }

    #[test]
    fn zero_readback_or_mismatch_retries_once_then_fails_at_two_attempts() {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).unwrap();
        let t0 = Utc::now();

        let EnqueueOutcome::ReadyToWrite(r1) = db
            .enqueue_switch(sample_req("retry-key", "macbook", 21), 5000, t0)
            .unwrap()
        else {
            panic!("expected ReadyToWrite");
        };

        // Attempt 1 write -> settling -> verifying -> readback returns 0 (invalid!)
        db.record_ddc_write_attempt(r1.id, "macbook", None, t0).unwrap();
        let t_v1 = t0 + Duration::milliseconds(5000);
        let v1 = db.begin_verifying_if_settled("macbook", 5000, t_v1).unwrap().unwrap();
        assert_eq!(v1.status, SwitchStatus::Verifying);

        let res1 = db
            .complete_verification(r1.id, "macbook", Some(0), None, t_v1)
            .unwrap();
        let VerifyOutcome::RetryWrite(retry_row) = res1 else {
            panic!("expected RetryWrite");
        };
        assert_eq!(retry_row.status, SwitchStatus::Writing);
        assert_eq!(retry_row.attempts, 1);
        assert_eq!(retry_row.observed_input, Some(0));

        // Attempt 2 write -> settling -> verifying -> readback returns 17 (still mismatched) -> Failed
        db.record_ddc_write_attempt(r1.id, "macbook", None, t_v1).unwrap();
        let t_v2 = t_v1 + Duration::milliseconds(5000);
        db.begin_verifying_if_settled("macbook", 5000, t_v2).unwrap().unwrap();
        let res2 = db
            .complete_verification(r1.id, "macbook", Some(17), None, t_v2)
            .unwrap();
        let VerifyOutcome::Failed(failed_row) = res2 else {
            panic!("expected Failed");
        };
        assert_eq!(failed_row.status, SwitchStatus::Failed);
        assert_eq!(failed_row.attempts, 2);
        assert_eq!(failed_row.observed_input, Some(17));
        assert!(failed_row.finished_at.is_some());
    }

    #[test]
    fn pending_successor_at_verifying_supersedes_old_and_promotes_latest() {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).unwrap();
        let t0 = Utc::now();

        let EnqueueOutcome::ReadyToWrite(r1) = db
            .enqueue_switch(sample_req("p1", "macbook", 21), 5000, t0)
            .unwrap()
        else {
            panic!("expected ReadyToWrite");
        };
        db.record_ddc_write_attempt(r1.id, "macbook", None, t0).unwrap();
        db.enqueue_switch(sample_req("p2", "win-desktop", 17), 5000, t0 + Duration::milliseconds(1000))
            .unwrap();

        let t_settle = t0 + Duration::milliseconds(5000);
        db.begin_verifying_if_settled("macbook", 5000, t_settle).unwrap().unwrap();
        let outcome = db
            .complete_verification(r1.id, "macbook", Some(21), None, t_settle)
            .unwrap();
        assert!(matches!(outcome, VerifyOutcome::SupersededByPending(_)));

        let promoted = db
            .promote_pending_if_ready("macbook", 5000, t_settle)
            .unwrap()
            .unwrap();
        assert_eq!(promoted.request_key, "p2");
        assert_eq!(promoted.status, SwitchStatus::Writing);
        assert_eq!(promoted.target_input, 17);
    }

    #[test]
    fn restart_recovery_marks_executing_failed_and_keeps_pending() {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).unwrap();
        let t0 = Utc::now();

        let EnqueueOutcome::ReadyToWrite(r1) = db
            .enqueue_switch(sample_req("crash-1", "macbook", 21), 5000, t0)
            .unwrap()
        else {
            panic!("expected ReadyToWrite");
        };
        db.record_ddc_write_attempt(r1.id, "macbook", None, t0).unwrap();
        db.enqueue_switch(sample_req("keep-pending", "win-desktop", 17), 5000, t0 + Duration::milliseconds(200))
            .unwrap();

        let recovered = db
            .recover_after_restart("macbook", t0 + Duration::seconds(10))
            .unwrap();
        assert_eq!(recovered, 1);

        let rows = db.list_recent_requests(10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].request_key, "keep-pending");
        assert_eq!(rows[0].status, SwitchStatus::Pending);
        assert_eq!(rows[1].request_key, "crash-1");
        assert_eq!(rows[1].status, SwitchStatus::Failed);
    }

    #[test]
    fn retention_prunes_only_terminal_rows_older_than_30_days() {
        let dir = tempfile::tempdir().unwrap();
        let db = StateDb::open(&dir.path().join("state.db")).unwrap();
        let old_ts = Utc::now() - Duration::days(31);

        let EnqueueOutcome::ReadyToWrite(r_old) = db
            .enqueue_switch(sample_req("old-done", "macbook", 21), 5000, old_ts)
            .unwrap()
        else {
            panic!("expected ReadyToWrite");
        };
        db.record_ddc_write_attempt(r_old.id, "macbook", None, old_ts).unwrap();
        let old_v = old_ts + Duration::seconds(6);
        db.begin_verifying_if_settled("macbook", 5000, old_v).unwrap().unwrap();
        db.complete_verification(r_old.id, "macbook", Some(21), None, old_v).unwrap();

        let now = Utc::now();
        db.enqueue_switch(sample_req("recent-writing", "win-desktop", 17), 5000, now)
            .unwrap();

        let deleted = db.prune_older_than_days(30, now).unwrap();
        assert_eq!(deleted, 1);
        let remaining = db.list_recent_requests(10).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].request_key, "recent-writing");
    }
}
