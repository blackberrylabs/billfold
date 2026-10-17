//! What Billfold finds, in its own words: the accounts a machine is wired to, the keys it
//! holds, the tokens it burned, the last quota a host wrote down — and the findings.
//! `Kind::severity` and `Kind::slug` are the tower's contract (see `docs/new-module.md`
//! in the tower); the mapping is written in the CHANGELOG.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bbl_report::Severity;
use serde::Serialize;

/// The kinds of finding. One slug each, one place on the four common severities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A key in clear in a file tracked by git.
    TrackedSecret,
    /// A key in clear in a file other users can read (mode wider than 0600).
    ExposedSecret,
    /// A key in clear, permissions fine: `secrets.json`, `.bashrc`, an ignored `.env`.
    ClearSecret,
    /// The same key (session fingerprint) in several tools or files.
    SharedKey,
    /// A token or session whose expiry is past.
    Expired,
    /// A host logged in on a subscription and an API key of the same provider in its
    /// environment: which one pays?
    DoubleCredit,
    /// A model consumed that the embedded price table does not know: tokens counted,
    /// dollars impossible.
    Unpriced,
    /// A usage log the host will prune within seven days.
    PruningSoon,
    /// The cost the host wrote differs from ours by more than 10 %.
    HostDisagrees,
    /// The last quota Codex wrote is at 80 % or more of a window.
    NearLimit,
    /// A credential file the host no longer uses.
    LegacyStore,
    /// A host present that writes no usage log at all.
    NoUsageLog,
    /// Secrets in the system keyring according to the sheet: nothing to read.
    InKeyring,
}

impl Kind {
    pub const ALL: [Kind; 13] = [
        Kind::TrackedSecret,
        Kind::ExposedSecret,
        Kind::ClearSecret,
        Kind::SharedKey,
        Kind::Expired,
        Kind::DoubleCredit,
        Kind::Unpriced,
        Kind::PruningSoon,
        Kind::HostDisagrees,
        Kind::NearLimit,
        Kind::LegacyStore,
        Kind::NoUsageLog,
        Kind::InKeyring,
    ];

    /// The slug, as it appears in `--json` and in the tower's report.
    pub fn slug(self) -> &'static str {
        match self {
            Kind::TrackedSecret => "tracked-secret",
            Kind::ExposedSecret => "exposed-secret",
            Kind::ClearSecret => "clear-secret",
            Kind::SharedKey => "shared-key",
            Kind::Expired => "expired",
            Kind::DoubleCredit => "double-credit",
            Kind::Unpriced => "unpriced",
            Kind::PruningSoon => "pruning-soon",
            Kind::HostDisagrees => "host-disagrees",
            Kind::NearLimit => "near-limit",
            Kind::LegacyStore => "legacy-store",
            Kind::NoUsageLog => "no-usage-log",
            Kind::InKeyring => "in-keyring",
        }
    }

    /// Where the kind sits on the four common severities.
    pub fn severity(self) -> Severity {
        match self {
            Kind::TrackedSecret | Kind::ExposedSecret => Severity::ActNow,
            Kind::ClearSecret
            | Kind::SharedKey
            | Kind::Expired
            | Kind::DoubleCredit
            | Kind::Unpriced => Severity::Review,
            Kind::PruningSoon | Kind::HostDisagrees | Kind::NearLimit | Kind::LegacyStore => {
                Severity::GoodToKnow
            }
            Kind::NoUsageLog | Kind::InKeyring => Severity::Info,
        }
    }

    /// Label as the CLI prints it.
    pub fn label(self) -> &'static str {
        match self {
            Kind::TrackedSecret => "TRACKED",
            Kind::ExposedSecret => "EXPOSED",
            Kind::ClearSecret => "IN CLEAR",
            Kind::SharedKey => "SHARED",
            Kind::Expired => "EXPIRED",
            Kind::DoubleCredit => "DOUBLE",
            Kind::Unpriced => "UNPRICED",
            Kind::PruningSoon => "PRUNING",
            Kind::HostDisagrees => "DISAGREES",
            Kind::NearLimit => "NEAR LIMIT",
            Kind::LegacyStore => "LEGACY",
            Kind::NoUsageLog => "NO LOG",
            Kind::InKeyring => "KEYRING",
        }
    }

    /// One sentence, for the app's "What it means".
    pub fn meaning(self) -> &'static str {
        match self {
            Kind::TrackedSecret => "A key in clear sits in a file git tracks: it is in the history of the repository, and of every clone and remote.",
            Kind::ExposedSecret => "A key in clear sits in a file other users of this machine can read.",
            Kind::ClearSecret => "A key is stored in clear text. The file's permissions are fine, but anything running as you can read it.",
            Kind::SharedKey => "The same key is used by several tools or stored in several places: rotating it means changing it everywhere, and one leak exposes all of them.",
            Kind::Expired => "The stored token or session has passed its expiry; the host will ask you to log in again, or is using something else.",
            Kind::DoubleCredit => "This host is logged in on a subscription and also sees an API key for the same provider: one of the two pays, and the host decides which.",
            Kind::Unpriced => "This model is in your logs but not in the embedded price table: its tokens are counted, its dollar equivalent is not.",
            Kind::PruningSoon => "The host deletes its usage logs after a retention period; what is older than that will soon be gone from this machine.",
            Kind::HostDisagrees => "The host wrote its own cost for these requests and it differs from Billfold's estimate: a different price table, a promotion, or a tier.",
            Kind::NearLimit => "The last time this host wrote its rate-limit state, a window was close to full.",
            Kind::LegacyStore => "This credential file belongs to an older version of the host; the current version stores its login elsewhere.",
            Kind::NoUsageLog => "This host keeps no per-request token record on disk; its usage lives only in the provider's dashboard.",
            Kind::InKeyring => "This host keeps its secrets in the system keyring, where Billfold does not look. Nothing to read, nothing exposed.",
        }
    }
}

/// How a host pays for what it sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Credit {
    /// Logged in on a plan; tokens have no marginal cost.
    Subscription { plan: Option<String> },
    /// An API key, pay-as-you-go.
    ApiKey,
    /// Stored in the system keyring: not read.
    Keyring,
    /// A file exists but says nothing readable about the credit.
    Unknown,
}

/// One account a host is wired to.
#[derive(Debug, Clone, Serialize)]
pub struct Account {
    pub host: String,
    pub host_name: String,
    pub provider: String,
    pub credit: Credit,
    /// The domain of the e-mail only (`gmail.com`), never the address.
    pub email_domain: Option<String>,
    pub organization: Option<String>,
    /// RFC 3339 when the host writes one.
    pub expires_at: Option<String>,
    pub expired: bool,
    pub path: PathBuf,
    /// Octal mode of the file (`600`), Unix only.
    pub mode: Option<String>,
    /// Modified time, RFC 3339.
    pub modified: Option<String>,
    pub legacy: bool,
    pub note: Option<String>,
}

/// Where a key was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeyPlace {
    /// The environment Billfold itself was started with.
    ProcessEnv,
    /// A shell start-up file (`~/.bashrc`, `~/.zshrc`, `~/.profile`).
    ShellRc,
    /// `~/.config/environment.d/*.conf`, `/etc/environment`.
    EnvironmentD,
    /// A project `.env*` or `.envrc`.
    Dotenv,
    /// A tool's own configuration or secrets file.
    ToolConfig,
    /// An MCP server definition's `env` block or header.
    McpConfig,
    /// `~/.netrc`, `.npmrc`, `pip.conf`, Docker, cloud CLIs.
    Other,
}

/// How git sees the file holding a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GitState {
    /// In the index of a repository.
    Tracked,
    /// Under a repository, matched by a `.gitignore`.
    Ignored,
    /// Under a repository, neither tracked nor ignored.
    Untracked,
    /// Not under a git repository.
    NotInRepo,
}

/// One provider key, masked.
#[derive(Debug, Clone, Serialize)]
pub struct Key {
    pub provider: String,
    pub provider_name: String,
    /// The variable or config key it was found under.
    pub name: String,
    /// Masked form (`sk-ant-api03-…igAA`), never more than four trailing characters.
    pub shown: String,
    /// Eight hex characters of an HMAC with a per-run salt: equal across places in one run,
    /// meaningless across runs.
    pub fingerprint: String,
    pub place: KeyPlace,
    pub path: PathBuf,
    pub line: Option<usize>,
    pub mode: Option<String>,
    /// The file is readable by group or others.
    pub exposed: bool,
    pub git: GitState,
    /// Hosts whose sheet says they read this file or this variable.
    pub used_by: Vec<String>,
    /// `true` when the value is a reference the host expands (`${VAR}`), listed for
    /// completeness and never counted as a key in clear.
    pub reference: bool,
}

/// Tokens of one request or one aggregated line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    /// Reasoning tokens when the host separates them (already included in `output`).
    pub reasoning: u64,
    pub requests: u64,
}

impl Tokens {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_write + self.cache_read
    }

    pub fn add(&mut self, o: &Tokens) {
        self.input += o.input;
        self.output += o.output;
        self.cache_write += o.cache_write;
        self.cache_read += o.cache_read;
        self.reasoning += o.reasoning;
        self.requests += o.requests;
    }

    /// Share of the prompt served from cache, in percent.
    pub fn cache_share(&self) -> Option<f64> {
        let prompt = self.input + self.cache_write + self.cache_read;
        if prompt == 0 {
            None
        } else {
            Some(self.cache_read as f64 * 100.0 / prompt as f64)
        }
    }
}

/// What the host says about how a request was billed, when it says anything.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Billing {
    /// Cache writes with a one-hour lifetime (Claude Code `ephemeral_1h_input_tokens`).
    pub cache_write_1h: u64,
    /// `fast` / `priority` service tier as the host logged it.
    pub tier: Option<String>,
    /// `inference_geo` (Claude Code), when not global.
    pub geo: Option<String>,
}

/// One line of the meter: a host, a model, a project, a day.
#[derive(Debug, Clone, Serialize)]
pub struct UsageLine {
    pub host: String,
    pub host_name: String,
    /// The model id as the host wrote it.
    pub model: String,
    pub provider: Option<String>,
    /// The project directory the host recorded, when it does.
    pub project: Option<PathBuf>,
    /// `YYYY-MM-DD`, local day.
    pub day: String,
    pub tokens: Tokens,
    pub billing: Billing,
    /// The cost the host computed itself, in USD, when it did.
    pub host_cost_usd: Option<f64>,
    /// Billfold's API-equivalent estimate, in USD, at the snapshot's prices.
    pub usd: Option<f64>,
    /// Why `usd` is `None` or approximate.
    pub pricing_note: Option<String>,
    pub credit: Credit,
    /// The log files these tokens came from.
    pub sources: Vec<PathBuf>,
}

/// The last rate-limit state a host wrote down (Codex).
#[derive(Debug, Clone, Serialize)]
pub struct Quota {
    pub host: String,
    pub host_name: String,
    pub window: String,
    pub used_percent: f64,
    pub window_minutes: Option<i64>,
    /// RFC 3339.
    pub resets_at: Option<String>,
    pub plan: Option<String>,
    /// When the host wrote it, RFC 3339.
    pub as_of: Option<String>,
    pub path: PathBuf,
}

/// One usage log file and what the host will do with it.
#[derive(Debug, Clone, Serialize)]
pub struct LogFile {
    pub host: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub modified: Option<String>,
    /// Days left before the host's retention deletes it, when documented.
    pub days_left: Option<i64>,
    pub requests: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostStatus {
    pub id: String,
    pub name: String,
    pub present: bool,
    /// Why it is deemed present (or what was looked for).
    pub presence: String,
    /// `true` when the sheet knows a usage log for it.
    pub has_usage_log: bool,
    pub accounts: usize,
    pub requests: u64,
    pub tokens: Tokens,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub kind: Kind,
    /// Stable across runs for the same thing.
    pub id: String,
    pub host: Option<String>,
    pub host_name: Option<String>,
    pub title: String,
    pub subject: String,
    pub detail: String,
    pub path: Option<PathBuf>,
    pub line: Option<usize>,
    /// What the person could do — shown, never run.
    pub hint: Option<String>,
    /// The host's own command to check (`claude` then `/status`).
    pub command: Option<String>,
    /// The page where the provider shows the same thing.
    pub url: Option<String>,
    /// The documentation or source the rule comes from.
    pub source_url: Option<String>,
}

/// The price table that priced the meter.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PriceInfo {
    pub name: String,
    pub as_of: String,
    pub license: String,
    pub attribution: String,
    pub models: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Inventory {
    pub home: PathBuf,
    /// First day of the window, `YYYY-MM-DD`.
    pub since: String,
    pub hosts: Vec<HostStatus>,
    pub accounts: Vec<Account>,
    pub keys: Vec<Key>,
    pub usage: Vec<UsageLine>,
    pub quotas: Vec<Quota>,
    pub logs: Vec<LogFile>,
    pub findings: Vec<Finding>,
    pub prices: PriceInfo,
    /// Hosts left out, with the reason.
    pub not_read: Vec<bbl_hostrules::rules::NotRead>,
    pub notes: Vec<String>,
    pub errors: Vec<String>,
}

impl Inventory {
    /// Tokens per host over the window.
    pub fn tokens_by_host(&self) -> BTreeMap<String, Tokens> {
        let mut m: BTreeMap<String, Tokens> = BTreeMap::new();
        for u in &self.usage {
            m.entry(u.host.clone()).or_default().add(&u.tokens);
        }
        m
    }

    pub fn total_tokens(&self) -> Tokens {
        let mut t = Tokens::default();
        for u in &self.usage {
            t.add(&u.tokens);
        }
        t
    }

    /// Dollars over the window for pay-as-you-go lines only.
    pub fn total_usd(&self) -> f64 {
        self.usage
            .iter()
            .filter(|u| !matches!(u.credit, Credit::Subscription { .. }))
            .filter_map(|u| u.usd)
            .sum()
    }

    pub fn providers(&self) -> Vec<String> {
        let mut v: Vec<String> = self.accounts.iter().map(|a| a.provider.clone()).collect();
        v.extend(
            self.keys
                .iter()
                .filter(|k| !k.reference)
                .map(|k| k.provider.clone()),
        );
        v.retain(|p| p != "unknown");
        v.sort();
        v.dedup();
        v
    }
}
