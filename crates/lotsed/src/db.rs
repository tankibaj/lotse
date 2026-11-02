//! SQLite persistence: workspaces, projects, sessions (metadata only), settings.
//!
//! Schema changes are appended to [`MIGRATIONS`]; `PRAGMA user_version` is the applied count.
//! A database written by a newer daemon is refused, never downgraded.

use lotse_proto::{
    AgentKind, NotificationMode, Project, ProjectId, SessionId, Settings, UnixMs, Workspace,
    WorkspaceId,
};
use rusqlite::{Connection, ErrorCode, OptionalExtension, params};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

pub const DEFAULT_WORKSPACE: &str = "w1";

const MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE workspaces (
         id TEXT PRIMARY KEY,
         name TEXT NOT NULL,
         created_at INTEGER NOT NULL
     );
     CREATE TABLE projects (
         id TEXT PRIMARY KEY,
         workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
         name TEXT NOT NULL,
         path TEXT NOT NULL UNIQUE,
         color TEXT NOT NULL,
         icon TEXT NOT NULL,
         is_git INTEGER NOT NULL,
         notifications TEXT NOT NULL,
         created_at INTEGER NOT NULL
     );
     CREATE TABLE sessions (
         id TEXT PRIMARY KEY,
         project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
         title TEXT NOT NULL,
         title_auto INTEGER NOT NULL,
         agent TEXT NOT NULL,
         command TEXT NOT NULL,
         created_at INTEGER NOT NULL,
         last_activity INTEGER NOT NULL
     );
     CREATE TABLE settings (
         key TEXT PRIMARY KEY,
         value TEXT NOT NULL
     );",
    // v2: workspaces become user-visible (names unique), transcripts the daemon has seen
    // (today's token usage survives a daemon restart)
    "CREATE UNIQUE INDEX workspaces_name ON workspaces(name COLLATE NOCASE);
     CREATE TABLE transcripts (
         path TEXT PRIMARY KEY,
         seen_at INTEGER NOT NULL
     );",
];

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database schema version {found} is newer than this daemon supports ({supported})")]
    TooNew { found: u32, supported: u32 },
    #[error("bad stored value: {0}")]
    Corrupt(String),
    #[error("a workspace with that name exists")]
    NameTaken,
    #[error("the workspace still has projects")]
    NotEmpty,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type DbResult<T> = Result<T, DbError>;

/// What survives of a session in the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub id: SessionId,
    pub project_id: ProjectId,
    pub title: String,
    pub title_auto: bool,
    pub agent: AgentKind,
    pub command: String,
    pub created_at: UnixMs,
    pub last_activity: UnixMs,
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Open (creating with mode 0600) and migrate.
    pub fn open(path: &Path) -> DbResult<Self> {
        crate::secure::open_private(path)?;
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> DbResult<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> DbResult<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn schema_version(&self) -> DbResult<u32> {
        Ok(self
            .conn()
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    // workspaces

    /// In creation order: the default workspace first.
    pub fn list_workspaces(&self) -> DbResult<Vec<Workspace>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare("SELECT id, name FROM workspaces ORDER BY created_at, rowid")?;
        let rows = stmt.query_map([], workspace_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn get_workspace(&self, id: &WorkspaceId) -> DbResult<Option<Workspace>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT id, name FROM workspaces WHERE id = ?1",
                [id.as_str()],
                workspace_from_row,
            )
            .optional()?)
    }

    /// `NameTaken` when another workspace has the name (case-insensitive).
    pub fn insert_workspace(&self, w: &Workspace, created_at: UnixMs) -> DbResult<()> {
        self.conn()
            .execute(
                "INSERT INTO workspaces (id, name, created_at) VALUES (?1, ?2, ?3)",
                params![w.id.as_str(), w.name, created_at as i64],
            )
            .map_err(name_taken)?;
        Ok(())
    }

    pub fn rename_workspace(&self, id: &WorkspaceId, name: &str) -> DbResult<bool> {
        let n = self
            .conn()
            .execute(
                "UPDATE workspaces SET name = ?2 WHERE id = ?1",
                params![id.as_str(), name],
            )
            .map_err(name_taken)?;
        Ok(n > 0)
    }

    /// Delete a workspace. Its projects go to `move_to` first (one transaction); without
    /// `move_to`, a workspace that still has projects is refused (`NotEmpty`): the foreign key
    /// would otherwise delete them with it.
    pub fn delete_workspace(
        &self,
        id: &WorkspaceId,
        move_to: Option<&WorkspaceId>,
    ) -> DbResult<bool> {
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let projects: i64 = tx.query_row(
            "SELECT count(*) FROM projects WHERE workspace_id = ?1",
            [id.as_str()],
            |r| r.get(0),
        )?;
        if projects > 0 {
            let Some(target) = move_to else {
                return Err(DbError::NotEmpty);
            };
            tx.execute(
                "UPDATE projects SET workspace_id = ?2 WHERE workspace_id = ?1",
                params![id.as_str(), target.as_str()],
            )?;
        }
        let n = tx.execute("DELETE FROM workspaces WHERE id = ?1", [id.as_str()])?;
        tx.commit()?;
        Ok(n > 0)
    }

    pub fn set_project_workspace(&self, project: &ProjectId, ws: &WorkspaceId) -> DbResult<bool> {
        let n = self.conn().execute(
            "UPDATE projects SET workspace_id = ?2 WHERE id = ?1",
            params![project.as_str(), ws.as_str()],
        )?;
        Ok(n > 0)
    }

    // transcripts the daemon has seen (usage)

    pub fn remember_transcript(&self, path: &str, seen_at: UnixMs) -> DbResult<()> {
        self.conn().execute(
            "INSERT INTO transcripts (path, seen_at) VALUES (?1, ?2)
             ON CONFLICT(path) DO UPDATE SET seen_at = excluded.seen_at",
            params![path, seen_at as i64],
        )?;
        Ok(())
    }

    /// Paths seen at or after `since`; older rows are dropped.
    pub fn recent_transcripts(&self, since: UnixMs) -> DbResult<Vec<String>> {
        let conn = self.conn();
        conn.execute("DELETE FROM transcripts WHERE seen_at < ?1", [since as i64])?;
        let mut stmt = conn.prepare("SELECT path FROM transcripts ORDER BY seen_at, path")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // projects

    pub fn list_projects(&self) -> DbResult<Vec<Project>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, workspace_id, name, path, color, icon, is_git, notifications
             FROM projects ORDER BY created_at, rowid",
        )?;
        let rows = stmt.query_map([], project_from_row)?;
        rows.map(|r| r?).collect()
    }

    pub fn get_project(&self, id: &ProjectId) -> DbResult<Option<Project>> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT id, workspace_id, name, path, color, icon, is_git, notifications
                 FROM projects WHERE id = ?1",
                [id.as_str()],
                project_from_row,
            )
            .optional()?;
        row.transpose()
    }

    pub fn project_by_path(&self, path: &str) -> DbResult<Option<Project>> {
        let conn = self.conn();
        let row = conn
            .query_row(
                "SELECT id, workspace_id, name, path, color, icon, is_git, notifications
                 FROM projects WHERE path = ?1",
                [path],
                project_from_row,
            )
            .optional()?;
        row.transpose()
    }

    pub fn insert_project(&self, p: &Project, created_at: UnixMs) -> DbResult<()> {
        let notifications =
            serde_json::to_string(&p.notifications).map_err(|e| DbError::Corrupt(e.to_string()))?;
        self.conn().execute(
            "INSERT INTO projects (id, workspace_id, name, path, color, icon, is_git, notifications, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                p.id.as_str(),
                p.workspace_id.as_str(),
                p.name,
                p.path,
                p.color,
                p.icon,
                p.is_git,
                notifications,
                created_at as i64
            ],
        )?;
        Ok(())
    }

    pub fn update_project(&self, p: &Project) -> DbResult<bool> {
        let notifications =
            serde_json::to_string(&p.notifications).map_err(|e| DbError::Corrupt(e.to_string()))?;
        let n = self.conn().execute(
            "UPDATE projects SET name = ?2, color = ?3, icon = ?4, is_git = ?5, notifications = ?6 WHERE id = ?1",
            params![p.id.as_str(), p.name, p.color, p.icon, p.is_git, notifications],
        )?;
        Ok(n > 0)
    }

    /// Also removes the project's sessions (foreign key cascade).
    pub fn delete_project(&self, id: &ProjectId) -> DbResult<bool> {
        let n = self
            .conn()
            .execute("DELETE FROM projects WHERE id = ?1", [id.as_str()])?;
        Ok(n > 0)
    }

    // sessions

    pub fn insert_session(&self, s: &SessionRow) -> DbResult<()> {
        self.conn().execute(
            "INSERT INTO sessions (id, project_id, title, title_auto, agent, command, created_at, last_activity)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                s.id.as_str(),
                s.project_id.as_str(),
                s.title,
                s.title_auto,
                agent_to_str(s.agent),
                s.command,
                s.created_at as i64,
                s.last_activity as i64
            ],
        )?;
        Ok(())
    }

    pub fn update_session_title(
        &self,
        id: &SessionId,
        title: &str,
        title_auto: bool,
    ) -> DbResult<()> {
        self.conn().execute(
            "UPDATE sessions SET title = ?2, title_auto = ?3 WHERE id = ?1",
            params![id.as_str(), title, title_auto],
        )?;
        Ok(())
    }

    pub fn delete_session(&self, id: &SessionId) -> DbResult<()> {
        self.conn()
            .execute("DELETE FROM sessions WHERE id = ?1", [id.as_str()])?;
        Ok(())
    }

    pub fn list_sessions(&self) -> DbResult<Vec<SessionRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, project_id, title, title_auto, agent, command, created_at, last_activity
             FROM sessions ORDER BY created_at, rowid",
        )?;
        let rows = stmt.query_map([], |r| {
            let agent: String = r.get(4)?;
            Ok((
                SessionRow {
                    id: SessionId::new(r.get::<_, String>(0)?),
                    project_id: ProjectId::new(r.get::<_, String>(1)?),
                    title: r.get(2)?,
                    title_auto: r.get(3)?,
                    agent: AgentKind::Terminal,
                    command: r.get(5)?,
                    created_at: r.get::<_, i64>(6)? as u64,
                    last_activity: r.get::<_, i64>(7)? as u64,
                },
                agent,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (mut s, agent) = row?;
            s.agent = agent_from_str(&agent)?;
            out.push(s);
        }
        Ok(out)
    }

    /// Sessions do not survive a daemon restart in Alpha (their PTYs are gone): drop the stale rows.
    pub fn prune_sessions(&self) -> DbResult<usize> {
        Ok(self.conn().execute("DELETE FROM sessions", [])?)
    }

    // settings

    pub fn load_settings(&self) -> DbResult<Settings> {
        let raw: Option<String> = self
            .conn()
            .query_row(
                "SELECT value FROM settings WHERE key = 'settings'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        match raw {
            None => Ok(Settings::default()),
            Some(raw) => {
                serde_json::from_str(&raw).map_err(|e| DbError::Corrupt(format!("settings: {e}")))
            }
        }
    }

    pub fn save_settings(&self, s: &Settings) -> DbResult<()> {
        let raw = serde_json::to_string(s).map_err(|e| DbError::Corrupt(e.to_string()))?;
        self.conn().execute(
            "INSERT INTO settings (key, value) VALUES ('settings', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [raw],
        )?;
        Ok(())
    }
}

fn migrate(conn: &Connection) -> DbResult<()> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let supported = MIGRATIONS.len() as u32;
    if current > supported {
        return Err(DbError::TooNew {
            found: current,
            supported,
        });
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as u32 + 1)?;
        tx.commit()?;
    }
    // the default workspace (Alpha UI shows exactly one)
    conn.execute(
        "INSERT OR IGNORE INTO workspaces (id, name, created_at) VALUES (?1, 'Default', 0)",
        [DEFAULT_WORKSPACE],
    )?;
    Ok(())
}

fn workspace_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: WorkspaceId::new(r.get::<_, String>(0)?),
        name: r.get(1)?,
    })
}

/// A unique-index violation of the workspace name.
fn name_taken(e: rusqlite::Error) -> DbError {
    match e {
        rusqlite::Error::SqliteFailure(f, _) if f.code == ErrorCode::ConstraintViolation => {
            DbError::NameTaken
        }
        other => DbError::Sqlite(other),
    }
}

fn project_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DbResult<Project>> {
    let id: String = r.get(0)?;
    let workspace_id: String = r.get(1)?;
    let name: String = r.get(2)?;
    let path: String = r.get(3)?;
    let color: String = r.get(4)?;
    let icon: String = r.get(5)?;
    let is_git: bool = r.get(6)?;
    let notifications: String = r.get(7)?;
    Ok(serde_json::from_str::<NotificationMode>(&notifications)
        .map_err(|e| DbError::Corrupt(format!("project notifications: {e}")))
        .map(|notifications| Project {
            id: ProjectId::new(id),
            workspace_id: WorkspaceId::new(workspace_id),
            name,
            path,
            color,
            icon,
            is_git,
            notifications,
        }))
}

fn agent_to_str(a: AgentKind) -> String {
    serde_json::to_value(a)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn agent_from_str(s: &str) -> DbResult<AgentKind> {
    serde_json::from_value(serde_json::Value::String(s.to_owned()))
        .map_err(|_| DbError::Corrupt(format!("unknown agent {s:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(id: &str, path: &str) -> Project {
        Project {
            id: ProjectId::new(id),
            workspace_id: WorkspaceId::new(DEFAULT_WORKSPACE),
            name: id.into(),
            path: path.into(),
            color: "teal".into(),
            icon: "folder".into(),
            is_git: false,
            notifications: NotificationMode::Global,
        }
    }

    fn session(id: &str, project: &str) -> SessionRow {
        SessionRow {
            id: SessionId::new(id),
            project_id: ProjectId::new(project),
            title: "t".into(),
            title_auto: true,
            agent: AgentKind::OpenCode,
            command: "opencode".into(),
            created_at: 1,
            last_activity: 2,
        }
    }

    #[test]
    fn migrates_fresh_database_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lotse.db");
        let db = Db::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), MIGRATIONS.len() as u32);
        db.insert_project(&project("p1", "/a"), 1).unwrap();
        drop(db);
        let db = Db::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), MIGRATIONS.len() as u32);
        assert_eq!(db.list_projects().unwrap().len(), 1);
    }

    #[test]
    fn refuses_a_newer_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lotse.db");
        {
            let c = Connection::open(&path).unwrap();
            c.pragma_update(None, "user_version", 99u32).unwrap();
        }
        assert!(matches!(
            Db::open(&path),
            Err(DbError::TooNew { found: 99, .. })
        ));
    }

    #[test]
    fn upgrades_from_an_older_version_without_losing_rows() {
        // simulate a database at v1 that a future v2 migration would extend
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1u32).unwrap();
        conn.execute("INSERT INTO workspaces VALUES ('w1', 'Default', 0)", [])
            .unwrap();
        migrate(&conn).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM workspaces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn project_and_session_round_trip_with_cascade() {
        let db = Db::open_in_memory().unwrap();
        let p = project("p1", "/a");
        db.insert_project(&p, 1).unwrap();
        assert_eq!(db.get_project(&p.id).unwrap(), Some(p.clone()));
        assert_eq!(db.project_by_path("/a").unwrap(), Some(p.clone()));
        assert!(
            db.insert_project(&project("p2", "/a"), 2).is_err(),
            "path is unique"
        );

        let s = session("s1", "p1");
        db.insert_session(&s).unwrap();
        assert_eq!(db.list_sessions().unwrap(), vec![s.clone()]);
        db.update_session_title(&s.id, "new", false).unwrap();
        assert_eq!(db.list_sessions().unwrap()[0].title, "new");
        assert!(!db.list_sessions().unwrap()[0].title_auto);

        let mut p2 = p.clone();
        p2.name = "renamed".into();
        p2.notifications = NotificationMode::Mute;
        assert!(db.update_project(&p2).unwrap());
        assert_eq!(
            db.get_project(&p.id).unwrap().unwrap().notifications,
            NotificationMode::Mute
        );

        assert!(db.delete_project(&p.id).unwrap());
        assert!(db.list_sessions().unwrap().is_empty(), "sessions cascade");
    }

    #[test]
    fn settings_default_then_persist() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.load_settings().unwrap(), Settings::default());
        let mut s = Settings::default();
        s.launch_commands.claude_code = "claude --model opus".into();
        db.save_settings(&s).unwrap();
        assert_eq!(db.load_settings().unwrap(), s);
        s.scrollback_bytes = 1 << 20;
        db.save_settings(&s).unwrap();
        assert_eq!(db.load_settings().unwrap(), s);
    }

    fn ws(id: &str, name: &str) -> Workspace {
        Workspace {
            id: WorkspaceId::new(id),
            name: name.into(),
        }
    }

    #[test]
    fn v2_migration_keeps_v1_rows_and_adds_unique_names_and_transcripts() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1u32).unwrap();
        conn.execute("INSERT INTO workspaces VALUES ('w1', 'Default', 0)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO projects VALUES ('p1','w1','n','/a','teal','folder',0,'{\"mode\":\"global\"}',1)",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
        let projects: i64 = conn
            .query_row("SELECT count(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(projects, 1, "v1 rows survive");
        assert!(
            conn.execute("INSERT INTO workspaces VALUES ('w2', 'default', 5)", [])
                .is_err(),
            "names are unique, case-insensitive"
        );
        conn.execute("INSERT INTO transcripts VALUES ('/x.jsonl', 1)", [])
            .unwrap();
    }

    #[test]
    fn workspaces_persist_rename_and_keep_creation_order() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lotse.db");
        let db = Db::open(&path).unwrap();
        assert_eq!(db.list_workspaces().unwrap(), vec![ws("w1", "Default")]);
        db.insert_workspace(&ws("w2", "Client work"), 10).unwrap();
        db.insert_workspace(&ws("w3", "Home"), 20).unwrap();
        assert!(matches!(
            db.insert_workspace(&ws("w4", "client WORK"), 30),
            Err(DbError::NameTaken)
        ));
        assert!(
            db.rename_workspace(&WorkspaceId::new("w3"), "Private")
                .unwrap()
        );
        assert!(matches!(
            db.rename_workspace(&WorkspaceId::new("w3"), "Default"),
            Err(DbError::NameTaken)
        ));
        assert!(!db.rename_workspace(&WorkspaceId::new("nope"), "X").unwrap());
        drop(db);
        let db = Db::open(&path).unwrap();
        let names: Vec<_> = db
            .list_workspaces()
            .unwrap()
            .into_iter()
            .map(|w| w.name)
            .collect();
        assert_eq!(names, ["Default", "Client work", "Private"]);
        assert_eq!(
            db.get_workspace(&WorkspaceId::new("w2")).unwrap(),
            Some(ws("w2", "Client work"))
        );
    }

    #[test]
    fn deleting_a_workspace_never_deletes_its_projects() {
        let db = Db::open_in_memory().unwrap();
        let (w1, w2) = (WorkspaceId::new("w1"), WorkspaceId::new("w2"));
        db.insert_workspace(&ws("w2", "Client work"), 10).unwrap();
        db.insert_project(&project("p1", "/a"), 1).unwrap();
        assert!(
            db.set_project_workspace(&ProjectId::new("p1"), &w2)
                .unwrap()
        );
        assert!(
            !db.set_project_workspace(&ProjectId::new("zz"), &w2)
                .unwrap()
        );
        assert_eq!(
            db.get_project(&ProjectId::new("p1"))
                .unwrap()
                .unwrap()
                .workspace_id,
            w2
        );

        // refused while it has projects: nothing changes
        assert!(matches!(
            db.delete_workspace(&w2, None),
            Err(DbError::NotEmpty)
        ));
        assert_eq!(db.list_workspaces().unwrap().len(), 2);
        assert_eq!(db.list_projects().unwrap().len(), 1);

        // moved to the default workspace in the same step
        assert!(db.delete_workspace(&w2, Some(&w1)).unwrap());
        assert_eq!(db.list_workspaces().unwrap(), vec![ws("w1", "Default")]);
        assert_eq!(
            db.get_project(&ProjectId::new("p1"))
                .unwrap()
                .unwrap()
                .workspace_id,
            w1
        );

        // an empty one needs no target
        db.insert_workspace(&ws("w3", "Empty"), 20).unwrap();
        assert!(db.delete_workspace(&WorkspaceId::new("w3"), None).unwrap());
        assert!(!db.delete_workspace(&WorkspaceId::new("w3"), None).unwrap());
    }

    #[test]
    fn transcripts_are_remembered_and_old_ones_dropped() {
        let db = Db::open_in_memory().unwrap();
        db.remember_transcript("/a.jsonl", 100).unwrap();
        db.remember_transcript("/b.jsonl", 200).unwrap();
        db.remember_transcript("/a.jsonl", 300).unwrap(); // seen again
        assert_eq!(db.recent_transcripts(0).unwrap(), ["/b.jsonl", "/a.jsonl"]);
        assert_eq!(db.recent_transcripts(250).unwrap(), ["/a.jsonl"]);
        assert_eq!(db.recent_transcripts(0).unwrap(), ["/a.jsonl"], "pruned");
    }

    #[test]
    fn prune_drops_stale_sessions_only() {
        let db = Db::open_in_memory().unwrap();
        db.insert_project(&project("p1", "/a"), 1).unwrap();
        db.insert_session(&session("s1", "p1")).unwrap();
        assert_eq!(db.prune_sessions().unwrap(), 1);
        assert_eq!(db.list_projects().unwrap().len(), 1);
    }
}
