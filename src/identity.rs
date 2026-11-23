//! Identities and access: who may use Ventosa and what each one may touch.
//! An open, local take on what an IdP gives a team (users, roles, lifecycle,
//! audit) for hybrid pipelines where humans and AI share the work.
//!
//! `$XDG_CONFIG_HOME/ventosa/identity.json` holds roles and users. Passwords are
//! PBKDF2-HMAC-SHA256 (`pbkdf2$<iterations>$<salt hex>$<hash hex>`); the engine's
//! `auth.py` checks the same file for HTTP Basic auth on every web app. With no
//! users the app runs open (single-user mode) and login is off.
//!
//! A role grants: graph control (the full desktop app), the apps it may open
//! (`*` = all), and a chat with a chosen set of tools. Users take a role and can
//! narrow (never widen) its chat tools. Lifecycle: enabled, an expiry date, the
//! last login; every change and login lands in `audit.jsonl`.

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;

pub const ITERATIONS: u32 = 120_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Role {
    pub id: String,
    pub title: String,
    /// The full desktop app: graph, queues, settings. Without it the user lands in the launcher.
    pub graph: bool,
    /// App ids it may open (`dashboard`, `uploads`, `teleprompter`, `labelling`, `studio`, `media-labelling`, `docs`, custom app ids); `*` = all.
    pub apps: Vec<String>,
    /// May edit the docs (wiki).
    pub docs_edit: bool,
    /// Gets the chat app in the launcher.
    pub chat: bool,
    /// Tools the chat may call (names from the tool layer); `*` = all.
    pub tools: Vec<String>,
    /// May manage users and roles.
    pub admin: bool,
    /// Connections the role may use (ids from Connections, a group `ai-*`, or `*`). Roles with graph control use all.
    pub connections: Vec<String>,
}

impl Default for Role {
    fn default() -> Self {
        Self { id: String::new(), title: String::new(), graph: false, apps: vec![], docs_edit: false, chat: false, tools: vec![], admin: false, connections: vec![] }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct User {
    pub name: String,
    pub display: String,
    pub email: String,
    pub pass: String,
    pub role: String,
    pub enabled: bool,
    /// Unix seconds; 0 = never expires.
    pub expires: u64,
    pub created: u64,
    pub last_login: u64,
    /// Narrows the role's chat tools; empty = the role's set.
    pub tools: Vec<String>,
    /// Workspaces this user sees; empty = all.
    pub workspaces: Vec<String>,
    /// Must change the password at next login.
    pub reset: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Identity {
    pub roles: Vec<Role>,
    pub users: Vec<User>,
}

pub fn default_roles() -> Vec<Role> {
    let all = || vec!["*".to_string()];
    vec![
        Role { id: "admin".into(), title: "admin · everything".into(), graph: true, apps: all(), docs_edit: true, chat: true, tools: all(), admin: true, connections: all() },
        Role { id: "operator".into(), title: "operator · runs the graphs".into(), graph: true, apps: all(), docs_edit: true, chat: true, tools: all(), admin: false, connections: all() },
        Role {
            id: "labeller".into(),
            title: "labeller · labelling only".into(),
            graph: false,
            apps: vec!["labelling".into(), "studio".into(), "docs".into()],
            docs_edit: false,
            chat: false,
            tools: vec![],
            admin: false,
            connections: vec![],
        },
        Role {
            id: "assistant".into(),
            title: "workload · chat with chosen tools".into(),
            graph: false,
            apps: vec!["docs".into(), "dashboard".into()],
            docs_edit: false,
            chat: true,
            tools: vec!["list_tasks".into(), "add_task".into(), "queue_list".into(), "workspace_status".into(), "file_read".into(), "files_list".into(), "help".into()],
            admin: false,
            // the AI providers the chat runs on, nothing else (no search, MCP, CLIs, apps)
            connections: vec!["ai-*".into()],
        },
    ]
}

pub fn path() -> PathBuf {
    std::env::var_os("VENTOSA_IDENTITY").map(PathBuf::from).unwrap_or_else(|| crate::config::default_path().with_file_name("identity.json"))
}

pub fn audit_path() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("ventosa").join("audit.jsonl")
}

pub fn load() -> Identity {
    let mut id: Identity = fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    if id.roles.is_empty() {
        id.roles = default_roles();
    }
    id
}

pub fn save(id: &Identity) -> io::Result<()> {
    let p = path();
    crate::save(&p, id)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Append one line to the audit log: `{at, actor, action, target, ok}`.
pub fn audit(actor: &str, action: &str, target: &str, ok: bool) {
    let p = audit_path();
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{}", json!({"at": crate::now(), "actor": actor, "action": action, "target": target, "ok": ok, "via": "app"}));
    }
}

/// The last `n` audit lines, newest first.
pub fn audit_tail(n: usize) -> Vec<serde_json::Value> {
    let s = fs::read_to_string(audit_path()).unwrap_or_default();
    s.lines().rev().filter_map(|l| serde_json::from_str(l).ok()).take(n).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    (0..s.len()).step_by(2).map(|i| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok())).collect()
}

fn salt() -> [u8; 16] {
    let mut b = [0u8; 16];
    if fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_err() {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        b.copy_from_slice(&n.to_le_bytes());
    }
    b
}

pub fn hash(password: &str) -> String {
    let s = salt();
    let mut out = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), &s, ITERATIONS, &mut out);
    format!("pbkdf2${ITERATIONS}${}${}", hex(&s), hex(&out))
}

pub fn verify(password: &str, stored: &str) -> bool {
    let parts: Vec<&str> = stored.split('$').collect();
    let [kind, iters, s, h] = parts.as_slice() else { return false };
    if *kind != "pbkdf2" {
        return false;
    }
    let (Ok(iters), Some(s), Some(h)) = (iters.parse::<u32>(), unhex(s), unhex(h)) else { return false };
    let mut out = vec![0u8; h.len()];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), &s, iters, &mut out);
    // constant-time compare
    out.len() == h.len() && out.iter().zip(&h).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
}

impl Identity {
    pub fn open_mode(&self) -> bool {
        self.users.is_empty()
    }
    pub fn role(&self, id: &str) -> Option<&Role> {
        self.roles.iter().find(|r| r.id == id)
    }
    pub fn user(&self, name: &str) -> Option<&User> {
        self.users.iter().find(|u| u.name == name)
    }
    /// Check a login. Ok(user) or the reason it failed (also audited).
    pub fn login(&mut self, name: &str, password: &str) -> Result<User, String> {
        let now = crate::now();
        let res = match self.users.iter_mut().find(|u| u.name == name.trim()) {
            None => Err("unknown user or wrong password".to_string()),
            Some(u) if !verify(password, &u.pass) => Err("unknown user or wrong password".into()),
            Some(u) if !u.enabled => Err("this account is disabled".into()),
            Some(u) if u.expires != 0 && u.expires < now => Err("this account has expired".into()),
            Some(u) => {
                u.last_login = now;
                Ok(u.clone())
            }
        };
        audit(name.trim(), "login", "app", res.is_ok());
        if res.is_ok() {
            let _ = save(self);
        }
        res
    }
    pub fn add_user(&mut self, actor: &str, name: &str, password: &str, role: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.contains(':') || name.contains(char::is_whitespace) {
            return Err("a user name has no spaces or ':'".into());
        }
        if password.len() < 8 {
            return Err("passwords are at least 8 characters".into());
        }
        if self.user(name).is_some() {
            return Err(format!("{name} exists"));
        }
        if self.role(role).is_none() {
            return Err(format!("no role {role}"));
        }
        self.users.push(User { name: name.into(), display: name.into(), pass: hash(password), role: role.into(), enabled: true, created: crate::now(), ..Default::default() });
        audit(actor, "user.create", name, true);
        save(self).map_err(|e| e.to_string())
    }
    pub fn set_password(&mut self, actor: &str, name: &str, password: &str) -> Result<(), String> {
        if password.len() < 8 {
            return Err("passwords are at least 8 characters".into());
        }
        let u = self.users.iter_mut().find(|u| u.name == name).ok_or("no such user")?;
        u.pass = hash(password);
        u.reset = false;
        audit(actor, "user.password", name, true);
        save(self).map_err(|e| e.to_string())
    }
}

/// What the person at the keyboard may do. `None` user = open mode (no users yet).
#[derive(Debug, Clone, Default)]
pub struct Access {
    pub user: Option<User>,
    pub role: Option<Role>,
}

impl Access {
    pub fn open() -> Self {
        Self::default()
    }
    pub fn for_user(id: &Identity, user: User) -> Self {
        let role = id.role(&user.role).cloned();
        Self { user: Some(user), role }
    }
    pub fn name(&self) -> &str {
        self.user.as_ref().map_or("local", |u| u.name.as_str())
    }
    fn role_or<T>(&self, open: T, f: impl FnOnce(&Role) -> T) -> T {
        match (&self.user, &self.role) {
            (None, _) => open,
            (Some(_), Some(r)) => f(r),
            (Some(_), None) => f(&Role::default()),
        }
    }
    pub fn can_graph(&self) -> bool {
        self.role_or(true, |r| r.graph)
    }
    pub fn is_admin(&self) -> bool {
        self.role_or(true, |r| r.admin)
    }
    pub fn can_app(&self, id: &str) -> bool {
        self.role_or(true, |r| r.graph || r.apps.iter().any(|a| a == "*" || a == id))
    }
    pub fn can_edit_docs(&self) -> bool {
        self.role_or(true, |r| r.docs_edit || r.admin)
    }
    pub fn can_chat(&self) -> bool {
        self.role_or(true, |r| r.chat)
    }
    /// Tools the chat may offer: the role's set, narrowed by the user's. Empty = none; `*` = all.
    pub fn tools(&self) -> Vec<String> {
        let role: Vec<String> = self.role_or(vec!["*".into()], |r| r.tools.clone());
        let user = self.user.as_ref().map(|u| u.tools.clone()).unwrap_or_default();
        if user.is_empty() {
            return role;
        }
        if role.iter().any(|t| t == "*") {
            return user;
        }
        user.into_iter().filter(|t| role.contains(t)).collect()
    }
    /// May this person use a connection (an AI provider, a search provider, an MCP server, a CLI, an app)?
    /// Graph control = all; others only what the role lists (`id`, `group-*` prefix, or `*`).
    pub fn can_connection(&self, id: &str) -> bool {
        self.role_or(true, |r| {
            r.graph
                || r.connections.iter().any(|c| c == "*" || c == id || c.strip_suffix('*').is_some_and(|p| id.starts_with(p)))
                // a chat role whose list names no AI provider runs on any (roles saved before connections existed)
                || (r.chat && id.starts_with("ai-") && !r.connections.iter().any(|c| c.starts_with("ai-")))
        })
    }
    /// May this person run an installer for a connection (sudo, npm, a terminal)? Admins only.
    pub fn can_install_connection(&self) -> bool {
        self.role_or(true, |r| r.admin)
    }
    pub fn sees_workspace(&self, ws: &str) -> bool {
        self.user.as_ref().is_none_or(|u| u.workspaces.is_empty() || u.workspaces.iter().any(|w| w == ws))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_verifies_and_rejects() {
        let h = hash("correct horse");
        assert!(verify("correct horse", &h));
        assert!(!verify("wrong", &h));
        assert!(!verify("x", "garbage"));
    }

    #[test]
    fn verifies_a_hash_made_by_the_engine() {
        // made by engine/auth.py's format (hashlib.pbkdf2_hmac), so the app and the web apps agree
        assert!(verify("cross-check", "pbkdf2$1000$000102030405060708090a0b0c0d0e0f$fd5ea7532066b3b07db9c6e09174f689f3d366132f68bdadd314d81b9798f6c4"));
    }

    #[test]
    fn user_tools_narrow_the_role() {
        let id = Identity { roles: default_roles(), users: vec![] };
        let u = User { name: "a".into(), role: "assistant".into(), tools: vec!["list_tasks".into(), "pipeline_write".into()], enabled: true, ..Default::default() };
        let a = Access::for_user(&id, u);
        assert_eq!(a.tools(), vec!["list_tasks".to_string()]);
        assert!(!a.can_graph());
        assert!(a.can_app("docs"));
        assert!(!a.can_app("labelling"));
        assert!(Access::open().can_graph());
    }
}
