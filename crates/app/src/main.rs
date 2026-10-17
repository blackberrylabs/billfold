//! Billfold desktop app (iced, on bbl-ui): the wiring, the keys and the meter in three tabs,
//! the findings most serious first, the detail of the selected one with its source. Reads
//! the same core as the CLI; writes only its preferences (`~/.config/billfold/settings.json`).
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{
    button, checkbox, column, container, horizontal_space, mouse_area, progress_bar, row,
    scrollable, text, text_input, Space,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Task, Theme};

use bbl_report::Severity;
use bbl_ui::palette::{alert, amber, good, muted};
use bbl_ui::widgets::{card, inner, kpi, pill, small_button, swatch};
use bbl_ui::{Lang, Prefs, Strings};
use billfold_core::model::{Credit, Finding, GitState, Inventory, Key, KeyPlace, Tokens};
use billfold_core::report::human;
use billfold_core::util::display_home;

const EN: &str = include_str!("../../../locales/en.json");
const FR: &str = include_str!("../../../locales/fr.json");

fn main() -> iced::Result {
    iced::application(App::title, App::update, App::view)
        .theme(App::theme)
        .window_size((1280.0, 820.0))
        .run_with(App::new)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Wiring,
    Keys,
    Meter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum By {
    Host,
    Model,
    Project,
    Day,
}

#[derive(Debug, Clone)]
enum Message {
    Loaded(Arc<Inventory>, u64),
    Rescan,
    SetLang(Lang),
    ToggleTheme,
    Select(String),
    FilterHost(Option<String>),
    ToggleInfo(bool),
    Search(String),
    SetTab(Tab),
    SetBy(By),
    SetDays(i64),
    OpenFile(String),
    OpenUrl(String),
    Copy(String),
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct Extra {
    /// The meter's window, in days (decision B4: 30 by default).
    #[serde(default)]
    since_days: Option<i64>,
}

struct App {
    prefs: Prefs<Extra>,
    lang: Lang,
    strings: Strings,
    dark: bool,
    home: PathBuf,
    roots: Vec<PathBuf>,
    days: i64,
    inventory: Option<Arc<Inventory>>,
    loading: bool,
    elapsed_ms: u64,
    selected: Option<String>,
    host_filter: Option<String>,
    show_info: bool,
    search: String,
    tab: Tab,
    by: By,
}

impl App {
    fn new() -> (App, Task<Message>) {
        let prefs: Prefs<Extra> = Prefs::load("billfold");
        let lang = prefs.initial_lang();
        let dark = prefs.initial_dark();
        let strings = Strings::load(EN, FR, lang);
        let (selected, roots, home_arg, days_arg, tab) = parse_args();
        let home = home_arg.unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/"))
        });
        let days = days_arg.or(prefs.extra.since_days).unwrap_or(30).max(1);
        let task = load(home.clone(), roots.clone(), days);
        (
            App {
                prefs,
                lang,
                strings,
                dark,
                home,
                roots,
                days,
                inventory: None,
                loading: true,
                elapsed_ms: 0,
                selected,
                host_filter: None,
                show_info: false,
                search: String::new(),
                tab,
                by: By::Model,
            },
            task,
        )
    }

    fn title(&self) -> String {
        self.strings.t("app.title", &[])
    }

    fn theme(&self) -> Theme {
        if self.dark {
            Theme::Dark
        } else {
            Theme::Light
        }
    }

    fn remember(&mut self) {
        self.prefs.remember(self.lang, self.dark);
        self.prefs.save();
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(inv, ms) => {
                if let Some(id) = &self.selected {
                    if let Some(f) = inv.findings.iter().find(|f| &f.id == id) {
                        if f.kind.severity() == Severity::Info {
                            self.show_info = true;
                        }
                    }
                }
                self.inventory = Some(inv);
                self.elapsed_ms = ms;
                self.loading = false;
            }
            Message::Rescan => {
                self.loading = true;
                return load(self.home.clone(), self.roots.clone(), self.days);
            }
            Message::SetLang(l) => {
                self.lang = l;
                self.strings = Strings::load(EN, FR, l);
                self.remember();
            }
            Message::ToggleTheme => {
                self.dark = !self.dark;
                self.remember();
            }
            Message::Select(id) => self.selected = Some(id),
            Message::FilterHost(h) => self.host_filter = h,
            Message::ToggleInfo(b) => self.show_info = b,
            Message::Search(s) => self.search = s,
            Message::SetTab(t) => self.tab = t,
            Message::SetBy(b) => self.by = b,
            Message::SetDays(d) => {
                self.days = d.max(1);
                self.prefs.extra.since_days = Some(self.days);
                self.prefs.save();
                self.loading = true;
                return load(self.home.clone(), self.roots.clone(), self.days);
            }
            Message::OpenFile(p) | Message::OpenUrl(p) => {
                // the one thing the app launches: the system opener, on a path or a URL —
                // never a command of ours
                let target = p
                    .rsplit_once(':')
                    .filter(|(_, l)| {
                        l.chars().all(|c| c.is_ascii_digit()) && !p.starts_with("http")
                    })
                    .map(|(a, _)| a.to_string())
                    .unwrap_or(p);
                let _ = std::process::Command::new("xdg-open").arg(&target).spawn();
            }
            Message::Copy(s) => return iced::clipboard::write(s),
        }
        Task::none()
    }

    // ---- helpers ---------------------------------------------------------------------

    fn sev_color(&self, s: Severity) -> Color {
        match s {
            Severity::ActNow => alert(self.dark),
            Severity::Review => amber(self.dark),
            Severity::GoodToKnow => good(self.dark),
            Severity::Info => muted(),
        }
    }

    fn credit(&self, c: &Credit) -> String {
        let s = &self.strings;
        match c {
            Credit::Subscription { plan: Some(p) } => s.t("credit.plan", &[("plan", p)]),
            Credit::Subscription { plan: None } => s.t("credit.subscription", &[]),
            Credit::ApiKey => s.t("credit.api_key", &[]),
            Credit::Keyring => s.t("credit.keyring", &[]),
            Credit::Unknown => s.t("credit.unknown", &[]),
        }
    }

    fn credit_color(&self, c: &Credit) -> Color {
        match c {
            Credit::Subscription { .. } => bbl_ui::palette::hex(0x2f6feb),
            Credit::ApiKey => muted(),
            Credit::Keyring => good(self.dark),
            Credit::Unknown => muted(),
        }
    }

    fn visible(&self) -> Vec<&Finding> {
        let Some(inv) = self.inventory.as_deref() else {
            return Vec::new();
        };
        let q = self.search.to_lowercase();
        inv.findings
            .iter()
            .filter(|f| self.show_info || f.kind.severity() != Severity::Info)
            .filter(|f| {
                self.host_filter
                    .as_ref()
                    .is_none_or(|h| f.host.as_ref() == Some(h))
            })
            .filter(|f| {
                q.is_empty()
                    || f.title.to_lowercase().contains(&q)
                    || f.subject.to_lowercase().contains(&q)
                    || f.detail.to_lowercase().contains(&q)
            })
            .collect()
    }

    fn selected_finding(&self) -> Option<&Finding> {
        let inv = self.inventory.as_deref()?;
        let id = self.selected.as_deref()?;
        inv.findings.iter().find(|f| f.id == id)
    }

    fn counts(inv: &Inventory) -> [usize; 4] {
        let mut c = [0usize; 4];
        for f in &inv.findings {
            c[(f.kind.severity().number() - 1) as usize] += 1;
        }
        c
    }

    // ---- view ------------------------------------------------------------------------

    fn view(&self) -> Element<'_, Message> {
        let s = &self.strings;
        let inv = self.inventory.as_deref();
        let body: Element<'_, Message> = match inv {
            Some(i) => row![self.view_left(i), self.view_list(i), self.view_detail(i)]
                .spacing(12)
                .height(Length::Fill)
                .into(),
            None => container(
                text(s.t("header.scanning", &[]))
                    .size(16)
                    .style(text::secondary),
            )
            .center(Length::Fill)
            .into(),
        };
        let status = match inv {
            Some(i) => s.t(
                "status.ready",
                &[
                    (
                        "hosts",
                        &i.hosts.iter().filter(|h| h.present).count().to_string(),
                    ),
                    ("accounts", &i.accounts.len().to_string()),
                    (
                        "keys",
                        &i.keys.iter().filter(|k| !k.reference).count().to_string(),
                    ),
                    ("requests", &i.total_tokens().requests.to_string()),
                ],
            ),
            None => s.t("header.scanning", &[]),
        };
        let theme_label = if self.dark {
            s.t("theme.light", &[])
        } else {
            s.t("theme.dark", &[])
        };
        let footer = bbl_ui::widgets::footer(
            status,
            self.lang,
            Message::SetLang,
            theme_label,
            Message::ToggleTheme,
        );
        let mut col = column![self.view_header(inv)].spacing(12).padding(14);
        if let Some(i) = inv {
            col = col.push(self.view_kpis(i));
        }
        col.push(body).push(footer).into()
    }

    fn view_header(&self, inv: Option<&Inventory>) -> Element<'_, Message> {
        let s = &self.strings;
        let sentence: Element<'_, Message> = match inv {
            None => text(s.t("headline.scanning", &[])).size(19).into(),
            Some(i) => {
                let present = i.hosts.iter().filter(|h| h.present).count();
                let [act, review, _, _] = Self::counts(i);
                let np = i.providers().len();
                let nk = i.keys.iter().filter(|k| !k.reference).count();
                let providers = if np == 1 {
                    s.t("headline.providers_one", &[])
                } else {
                    s.t("headline.providers", &[("n", &np.to_string())])
                };
                let keys = if nk == 1 {
                    s.t("headline.keys_one", &[])
                } else {
                    s.t("headline.keys", &[("n", &nk.to_string())])
                };
                let tokens = human(i.total_tokens().total());
                let usd = format!("${:.2}", i.total_usd());
                let days = self.days.to_string();
                let things = |n: usize| {
                    if n == 1 {
                        s.t("headline.things_one", &[])
                    } else {
                        s.t("headline.things", &[("n", &n.to_string())])
                    }
                };
                let args_base = [
                    ("providers", providers.as_str()),
                    ("keys", keys.as_str()),
                    ("tokens", tokens.as_str()),
                    ("days", days.as_str()),
                    ("usd", usd.as_str()),
                ];
                if present == 0 {
                    text(s.t("headline.none", &[])).size(19).into()
                } else if act == 0 && review == 0 {
                    text(s.t("headline.clear", &args_base)).size(19).into()
                } else if act == 0 {
                    let r = review.to_string();
                    let mut a = args_base.to_vec();
                    a.push(("review", r.as_str()));
                    text(s.t("headline.review", &a)).size(19).into()
                } else {
                    let r = review.to_string();
                    let marker = things(act);
                    let mut a = args_base.to_vec();
                    a.push(("act", marker.as_str()));
                    a.push(("review", r.as_str()));
                    let full = s.t("headline.act", &a);
                    match full.find(&marker) {
                        Some(i) => {
                            let (x, rest) = full.split_at(i);
                            let (y, z) = rest.split_at(marker.len());
                            row![
                                text(x.to_string()).size(19),
                                text(y.to_string()).size(19).color(alert(self.dark)),
                                text(z.to_string()).size(19)
                            ]
                            .into()
                        }
                        None => text(full).size(19).into(),
                    }
                }
            }
        };
        let when = if self.loading {
            s.t("header.scanning", &[])
        } else {
            s.t(
                "header.read",
                &[("s", &format!("{:.1}", self.elapsed_ms as f64 / 1000.0))],
            )
        };
        let rescan: Element<'_, Message> = if self.loading {
            button(text(s.t("header.scanning", &[])).size(12))
                .style(button::secondary)
                .into()
        } else {
            button(text(s.t("header.rescan", &[])).size(12))
                .style(button::primary)
                .on_press(Message::Rescan)
                .into()
        };
        let mut r = row![sentence, horizontal_space()]
            .spacing(10)
            .align_y(Alignment::Center);
        for d in [30i64, 90, 365] {
            r = r.push(chip(
                s.t("header.days", &[("n", &d.to_string())]),
                self.days == d,
                Message::SetDays(d),
            ));
        }
        r.push(text(when).size(12).style(text::secondary))
            .push(rescan)
            .into()
    }

    fn view_kpis(&self, inv: &Inventory) -> Element<'_, Message> {
        let s = &self.strings;
        let providers = inv.providers();
        let ps = billfold_core::providers::embedded();
        let names: Vec<String> = providers
            .iter()
            .map(|p| ps.name_of(p).split(" (").next().unwrap_or(p).to_string())
            .collect();
        let keys: Vec<&Key> = inv.keys.iter().filter(|k| !k.reference).collect();
        let tracked = keys.iter().filter(|k| k.git == GitState::Tracked).count();
        let clear = keys
            .iter()
            .filter(|k| k.git != GitState::Tracked && k.place != KeyPlace::ProcessEnv)
            .count();
        let mut fps: BTreeMap<&str, usize> = BTreeMap::new();
        for k in &keys {
            *fps.entry(&k.fingerprint).or_default() += 1;
        }
        let shared = fps.values().filter(|n| **n > 1).count();
        let t = inv.total_tokens();
        let plan_usd: f64 = inv
            .usage
            .iter()
            .filter(|u| matches!(u.credit, Credit::Subscription { .. }))
            .filter_map(|u| u.usd)
            .sum();
        let [act, _, _, _] = Self::counts(inv);
        row![
            kpi(
                s.t("kpi.providers", &[]),
                providers.len().to_string(),
                if names.is_empty() {
                    s.t("kpi.providers.none", &[])
                } else {
                    s.t("kpi.providers.sub", &[("list", &names.join(" · "))])
                },
                None
            ),
            kpi(
                s.t("kpi.keys", &[]),
                keys.len().to_string(),
                s.t(
                    "kpi.keys.sub",
                    &[
                        ("tracked", &tracked.to_string()),
                        ("clear", &clear.to_string()),
                        ("shared", &shared.to_string())
                    ]
                ),
                if tracked > 0 || act > 0 {
                    Some(alert(self.dark))
                } else {
                    None
                }
            ),
            kpi(
                s.t("kpi.tokens", &[("since", &inv.since)]),
                human(t.total()),
                if t.requests == 0 {
                    s.t("kpi.tokens.none", &[])
                } else {
                    s.t(
                        "kpi.tokens.sub",
                        &[
                            ("requests", &t.requests.to_string()),
                            (
                                "cache",
                                &t.cache_share()
                                    .map(|c| format!("{c:.0} %"))
                                    .unwrap_or_else(|| "–".into()),
                            ),
                        ],
                    )
                },
                None
            ),
            kpi(
                s.t("kpi.usd", &[]),
                format!("${:.2}", inv.total_usd()),
                s.t(
                    "kpi.usd.sub",
                    &[
                        ("plan", &format!("${plan_usd:.2}")),
                        (
                            "as_of",
                            inv.prices.as_of.get(..10).unwrap_or(&inv.prices.as_of)
                        ),
                        ("source", &inv.prices.name)
                    ]
                ),
                None
            ),
        ]
        .spacing(12)
        .into()
    }

    // ---- left column: tabs -----------------------------------------------------------

    fn view_left(&self, inv: &Inventory) -> Element<'_, Message> {
        let s = &self.strings;
        let tab = |t: Tab, key: &str| -> Element<'_, Message> {
            let on = self.tab == t;
            let b = button(text(s.t(key, &[])).size(12))
                .padding([4, 10])
                .on_press(Message::SetTab(t));
            if on {
                b.style(button::primary).into()
            } else {
                b.style(button::text).into()
            }
        };
        let tabs = row![
            tab(Tab::Wiring, "tab.wiring"),
            tab(Tab::Keys, "tab.keys"),
            tab(Tab::Meter, "tab.meter")
        ]
        .spacing(4);
        let content: Element<'_, Message> = match self.tab {
            Tab::Wiring => self.view_wiring(inv),
            Tab::Keys => self.view_keys(inv),
            Tab::Meter => self.view_meter(inv),
        };
        card(
            column![
                tabs,
                scrollable(container(content).padding(iced::Padding {
                    right: 12.0,
                    ..Default::default()
                }))
                .height(Length::Fill)
            ]
            .spacing(8)
            .into(),
        )
        .width(Length::FillPortion(5))
        .height(Length::Fill)
        .into()
    }

    fn view_wiring(&self, inv: &Inventory) -> Element<'_, Message> {
        let s = &self.strings;
        let mut col =
            column![text(s.t("wiring.sub", &[])).size(11).style(text::secondary)].spacing(8);
        let by_host = inv.tokens_by_host();
        for h in inv.hosts.iter().filter(|h| h.present) {
            let t = by_host.get(&h.id).copied().unwrap_or_default();
            let right = if t.requests > 0 {
                s.t(
                    "wiring.requests",
                    &[
                        ("n", &t.requests.to_string()),
                        ("tokens", &human(t.total())),
                    ],
                )
            } else if !h.has_usage_log {
                s.t("wiring.no_log", &[])
            } else {
                s.t("wiring.no_usage", &[])
            };
            let mut hc = column![row![
                text(h.name.clone()).size(13),
                text(shorten(&h.presence, 40))
                    .size(11)
                    .style(text::secondary)
                    .width(Length::Fill),
                text(right).size(11).style(text::secondary)
            ]
            .spacing(8)
            .align_y(Alignment::Center)]
            .spacing(4);
            let accounts: Vec<_> = inv.accounts.iter().filter(|a| a.host == h.id).collect();
            if accounts.is_empty() {
                hc = hc.push(
                    text(s.t("wiring.no_account", &[]))
                        .size(11)
                        .style(text::secondary),
                );
            }
            for a in accounts {
                let ps = billfold_core::providers::embedded();
                let mut who = ps
                    .name_of(&a.provider)
                    .split(" (")
                    .next()
                    .unwrap_or(&a.provider)
                    .to_string();
                if let Some(d) = &a.email_domain {
                    who.push_str(&format!(" · @{d}"));
                }
                if let Some(o) = &a.organization {
                    who.push_str(&format!(" · {o}"));
                }
                let file = if a.path.to_string_lossy().starts_with("keyring:") {
                    a.path
                        .to_string_lossy()
                        .trim_start_matches("keyring:")
                        .to_string()
                } else {
                    a.path
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_default()
                };
                let mut tags = row![text(file)
                    .size(11)
                    .font(iced::Font::MONOSPACE)
                    .style(text::secondary)]
                .spacing(6)
                .align_y(Alignment::Center);
                if a.legacy {
                    tags = tags.push(pill(s.t("wiring.no_longer_read", &[]), muted()));
                } else if matches!(a.credit, Credit::Keyring) {
                    tags = tags.push(pill(s.t("wiring.not_read", &[]), muted()));
                } else if let Some(e) = &a.expires_at {
                    let d = e.get(..10).unwrap_or(e).to_string();
                    tags = tags.push(if a.expired {
                        pill(s.t("wiring.expired", &[("date", &d)]), alert(self.dark))
                    } else {
                        pill(s.t("wiring.expires", &[("date", &d)]), muted())
                    });
                }
                // a key of the same provider in the shell environment (double credit)
                if matches!(a.credit, Credit::Subscription { .. }) {
                    if let Some(k) = inv.keys.iter().find(|k| {
                        k.provider == a.provider
                            && !k.reference
                            && matches!(
                                k.place,
                                KeyPlace::ProcessEnv | KeyPlace::ShellRc | KeyPlace::EnvironmentD
                            )
                    }) {
                        let w = if k.place == KeyPlace::ProcessEnv {
                            s.t("keys.environment", &[])
                        } else {
                            display_home(&k.path, &inv.home)
                        };
                        tags = tags.push(pill(
                            s.t("wiring.also_key", &[("where", &w)]),
                            alert(self.dark),
                        ));
                    }
                }
                let r = column![
                    row![
                        container(pill(
                            if a.legacy {
                                s.t("wiring.legacy", &[])
                            } else {
                                self.credit(&a.credit)
                            },
                            if a.legacy {
                                amber(self.dark)
                            } else {
                                self.credit_color(&a.credit)
                            }
                        ))
                        .width(Length::Fixed(96.0)),
                        text(who).size(12),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    container(tags).padding(iced::Padding {
                        left: 104.0,
                        ..Default::default()
                    }),
                ]
                .spacing(2);
                hc = hc.push(r);
            }
            let quotas: Vec<_> = inv.quotas.iter().filter(|q| q.host == h.id).collect();
            if !quotas.is_empty() {
                let mut qr = row![text(s.t("wiring.quota", &[]))
                    .size(11)
                    .style(text::secondary)
                    .width(Length::Fixed(46.0))]
                .spacing(10)
                .align_y(Alignment::Center);
                for q in &quotas {
                    let win = match q.window_minutes {
                        Some(m) if m >= 1440 => format!("{} d", m / 1440),
                        Some(m) if m >= 60 => format!("{} h", m / 60),
                        Some(m) => format!("{m} min"),
                        None => q.window.clone(),
                    };
                    let bar = progress_bar(0.0..=100.0, q.used_percent as f32)
                        .height(Length::Fixed(6.0))
                        .width(Length::Fixed(110.0));
                    let pct = text(format!("{:.0} %", q.used_percent)).size(11).color(
                        if q.used_percent >= 80.0 {
                            alert(self.dark)
                        } else {
                            muted()
                        },
                    );
                    qr = qr.push(
                        row![text(win).size(11), bar, pct]
                            .spacing(6)
                            .align_y(Alignment::Center),
                    );
                }
                hc = hc.push(qr);
                if let Some(t) = quotas[0].as_of.as_ref() {
                    hc = hc.push(
                        container(
                            text(s.t(
                                "wiring.quota_as_written",
                                &[("when", t.get(..16).unwrap_or(t).replace('T', " ").as_str())],
                            ))
                            .size(10)
                            .style(text::secondary),
                        )
                        .padding(iced::Padding {
                            left: 56.0,
                            ..Default::default()
                        }),
                    );
                }
            }
            col = col.push(inner(hc.into()));
        }
        col.into()
    }

    fn view_keys(&self, inv: &Inventory) -> Element<'_, Message> {
        let s = &self.strings;
        let mut col =
            column![text(s.t("keys.sub", &[])).size(11).style(text::secondary)].spacing(4);
        if inv.keys.is_empty() {
            return col.push(text(s.t("keys.none", &[])).size(12)).into();
        }
        let head = row![
            text(s.t("keys.col.provider", &[]))
                .size(11)
                .style(text::secondary)
                .width(Length::Fixed(78.0)),
            text(s.t("keys.col.name", &[]))
                .size(11)
                .style(text::secondary)
                .width(Length::Fixed(140.0)),
            text(s.t("keys.col.value", &[]))
                .size(11)
                .style(text::secondary)
                .width(Length::Fill),
            text(s.t("keys.col.git", &[]))
                .size(11)
                .style(text::secondary)
                .width(Length::Fixed(72.0)),
        ]
        .spacing(8);
        col = col.push(head);
        let ps = billfold_core::providers::embedded();
        for k in &inv.keys {
            let git = if k.reference {
                (s.t("keys.reference", &[]), muted())
            } else {
                match k.git {
                    GitState::Tracked => (s.t("keys.git.tracked", &[]), alert(self.dark)),
                    GitState::Ignored => (s.t("keys.git.ignored", &[]), muted()),
                    GitState::Untracked => (s.t("keys.git.untracked", &[]), amber(self.dark)),
                    GitState::NotInRepo => (s.t("keys.git.none", &[]), muted()),
                }
            };
            let mut place = if k.place == KeyPlace::ProcessEnv {
                s.t("keys.environment", &[])
            } else {
                display_home(&k.path, &inv.home)
            };
            if let Some(l) = k.line {
                place.push_str(&format!(":{l}"));
            }
            if let Some(m) = &k.mode {
                place.push_str(&format!(" · {m}"));
            }
            if !k.used_by.is_empty() {
                place.push_str(&format!(
                    " · {}",
                    s.t("keys.read_by", &[("hosts", &k.used_by.join(", "))])
                ));
            }
            let mut first = row![
                text(
                    ps.name_of(&k.provider)
                        .split(" (")
                        .next()
                        .unwrap_or(&k.provider)
                        .to_string()
                )
                .size(12)
                .width(Length::Fixed(78.0)),
                text(shorten(&k.name, 22))
                    .size(11)
                    .font(iced::Font::MONOSPACE)
                    .width(Length::Fixed(140.0)),
                text(k.shown.clone())
                    .size(11)
                    .font(iced::Font::MONOSPACE)
                    .width(Length::Fill),
                container(pill(git.0, git.1)).width(Length::Fixed(72.0)),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            if k.exposed {
                first = first.push(pill(s.t("keys.exposed", &[]), alert(self.dark)));
            }
            col = col.push(
                column![
                    first,
                    container(text(place).size(10).style(text::secondary)).padding(iced::Padding {
                        left: 86.0,
                        bottom: 4.0,
                        ..Default::default()
                    })
                ]
                .spacing(1),
            );
        }
        let mut fps: BTreeMap<&str, usize> = BTreeMap::new();
        for k in inv.keys.iter().filter(|k| !k.fingerprint.is_empty()) {
            *fps.entry(&k.fingerprint).or_default() += 1;
        }
        let shared = fps.values().filter(|n| **n > 1).count();
        if shared > 0 {
            col = col.push(Space::with_height(6));
            col = col.push(
                text(s.t("keys.shared", &[("n", &shared.to_string())]))
                    .size(11)
                    .style(text::secondary),
            );
        }
        col.into()
    }

    fn view_meter(&self, inv: &Inventory) -> Element<'_, Message> {
        let s = &self.strings;
        let mut head = row![text(s.t("meter.sub", &[])).size(11).style(text::secondary)]
            .spacing(6)
            .align_y(Alignment::Center);
        for (b, key) in [
            (By::Host, "meter.by.host"),
            (By::Model, "meter.by.model"),
            (By::Project, "meter.by.project"),
            (By::Day, "meter.by.day"),
        ] {
            head = head.push(chip(s.t(key, &[]), self.by == b, Message::SetBy(b)));
        }
        head = head.push(horizontal_space());
        head = head.push(
            text(s.t("meter.since", &[("since", &inv.since)]))
                .size(11)
                .style(text::secondary),
        );
        let mut col = column![head].spacing(6);
        if inv.usage.is_empty() {
            return col
                .push(text(s.t("meter.none", &[("since", &inv.since)])).size(12))
                .into();
        }
        // aggregate
        // tokens, our dollars, the host's dollars, on a plan, some lines unpriced, notes
        type Group = (Tokens, Option<f64>, f64, bool, bool, Vec<String>);
        let mut groups: BTreeMap<String, Group> = BTreeMap::new();
        for u in &inv.usage {
            let key = match self.by {
                By::Host => u.host_name.clone(),
                By::Model => format!("{} · {}", u.host_name, u.model),
                By::Project => format!(
                    "{} · {}",
                    u.host_name,
                    u.project
                        .as_ref()
                        .map(|p| display_home(p, &inv.home))
                        .unwrap_or_else(|| s.t("meter.no_project", &[]))
                ),
                By::Day => format!("{} · {}", u.day, u.host_name),
            };
            let g = groups.entry(key).or_insert((
                Tokens::default(),
                None,
                0.0,
                false,
                false,
                Vec::new(),
            ));
            g.0.add(&u.tokens);
            if let Some(d) = u.usd {
                g.1 = Some(g.1.unwrap_or(0.0) + d);
            } else {
                g.4 = true;
            }
            g.2 += u.host_cost_usd.unwrap_or(0.0);
            g.3 |= matches!(u.credit, Credit::Subscription { .. });
            if let Some(n) = &u.pricing_note {
                for part in n.split("; ") {
                    if part != "included in plan"
                        && part != "model not in the price table"
                        && !g.5.contains(&part.to_string())
                    {
                        g.5.push(part.to_string());
                    }
                }
            }
        }
        let w = |x: f32| Length::Fixed(x);
        let th = |k: &str| text(s.t(k, &[])).size(11).style(text::secondary);
        col = col.push(
            row![
                text("").width(Length::Fill),
                th("meter.col.reqs").width(w(44.0)),
                th("meter.col.usd").width(w(96.0))
            ]
            .spacing(6),
        );
        for (k, (t, usd, host_cost, plan, unpriced, notes)) in &groups {
            let money: Element<'_, Message> = match (usd, plan, unpriced) {
                (None, _, _) => container(pill(s.t("meter.unpriced", &[]), alert(self.dark)))
                    .width(w(96.0))
                    .into(),
                (Some(_), _, _) if notes.iter().any(|n| n == "free model") => {
                    container(pill(s.t("meter.free", &[]), bbl_ui::palette::hex(0x2f6feb)))
                        .width(w(96.0))
                        .into()
                }
                (Some(d), true, _) => row![
                    pill(s.t("meter.plan", &[]), bbl_ui::palette::hex(0x2f6feb)),
                    text(format!("(${d:.2})")).size(12)
                ]
                .spacing(4)
                .width(w(96.0))
                .align_y(Alignment::Center)
                .into(),
                (Some(d), false, _) => text(format!("${d:.2}")).size(12).width(w(96.0)).into(),
            };
            col = col.push(
                row![
                    text(k.clone()).size(12).width(Length::Fill),
                    text(t.requests.to_string()).size(12).width(w(44.0)),
                    money
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
            let mut parts = vec![
                format!("{} {}", s.t("meter.col.input", &[]), human(t.input)),
                format!("{} {}", s.t("meter.col.output", &[]), human(t.output)),
                format!("{} {}", s.t("meter.col.cache_w", &[]), human(t.cache_write)),
                format!("{} {}", s.t("meter.col.cache_r", &[]), human(t.cache_read)),
            ];
            if let Some(c) = t.cache_share() {
                parts.push(format!("{} {c:.0} %", s.t("meter.col.cache", &[])));
            }
            if *host_cost > 0.0 {
                parts.push(s.t("meter.host_cost", &[("usd", &format!("${host_cost:.2}"))]));
            }
            parts.extend(notes.iter().cloned());
            if *unpriced && usd.is_some() {
                parts.push(s.t("meter.unpriced", &[]));
            }
            col = col.push(
                container(text(parts.join(" · ")).size(10).style(text::secondary)).padding(
                    iced::Padding {
                        left: 12.0,
                        bottom: 4.0,
                        ..Default::default()
                    },
                ),
            );
        }
        // cache share per host
        col = col.push(Space::with_height(8));
        col = col.push(
            text(s.t("meter.cache_share", &[]).to_uppercase())
                .size(11)
                .style(text::secondary),
        );
        for (host, t) in inv.tokens_by_host() {
            let name = inv
                .hosts
                .iter()
                .find(|h| h.id == host)
                .map(|h| h.name.clone())
                .unwrap_or(host);
            let share = t.cache_share().unwrap_or(0.0);
            col = col.push(
                row![
                    text(name).size(12).width(w(110.0)),
                    progress_bar(0.0..=100.0, share as f32)
                        .height(Length::Fixed(8.0))
                        .width(w(220.0)),
                    text(format!("{share:.0} %"))
                        .size(11)
                        .style(text::secondary)
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        // log files
        let soon: Vec<_> = inv
            .logs
            .iter()
            .filter(|l| l.days_left.is_some_and(|d| d <= 7))
            .collect();
        if !soon.is_empty() {
            col = col.push(Space::with_height(8));
            col = col.push(
                text(s.t("meter.logs", &[]).to_uppercase())
                    .size(11)
                    .style(text::secondary),
            );
            for l in soon.iter().take(8) {
                let host = inv
                    .hosts
                    .iter()
                    .find(|h| h.id == l.host)
                    .map(|h| h.name.clone())
                    .unwrap_or_default();
                col = col.push(
                    row![
                        text(display_home(&l.path, &inv.home))
                            .size(10)
                            .font(iced::Font::MONOSPACE)
                            .width(Length::Fill),
                        pill(
                            s.t(
                                "meter.deleted_in",
                                &[
                                    ("host", &host),
                                    ("days", &l.days_left.unwrap_or(0).max(0).to_string())
                                ]
                            ),
                            amber(self.dark)
                        )
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                );
            }
        }
        col = col.push(Space::with_height(8));
        col = col.push(
            text(s.t(
                "meter.note",
                &[
                    (
                        "as_of",
                        inv.prices.as_of.get(..10).unwrap_or(&inv.prices.as_of),
                    ),
                    ("source", &inv.prices.name),
                    ("license", &inv.prices.license),
                ],
            ))
            .size(10)
            .style(text::secondary),
        );
        col.into()
    }

    // ---- findings list -----------------------------------------------------------------

    fn view_list(&self, inv: &Inventory) -> Element<'_, Message> {
        let s = &self.strings;
        let visible = self.visible();
        let mut hosts: Vec<(&str, &str)> = inv
            .findings
            .iter()
            .filter_map(|f| Some((f.host.as_deref()?, f.host_name.as_deref()?)))
            .collect();
        hosts.sort();
        hosts.dedup();
        let mut all: Vec<Element<'_, Message>> = vec![chip(
            s.t("list.all_hosts", &[]),
            self.host_filter.is_none(),
            Message::FilterHost(None),
        )];
        for (id, name) in hosts {
            all.push(chip(
                short_host(name),
                self.host_filter.as_deref() == Some(id),
                Message::FilterHost(Some(id.to_string())),
            ));
        }
        let mut chips = column![].spacing(4);
        let mut line = row![].spacing(6);
        for (i, c) in all.into_iter().enumerate() {
            if i > 0 && i % 5 == 0 {
                chips = chips.push(line);
                line = row![].spacing(6);
            }
            line = line.push(c);
        }
        chips = chips.push(line);
        let controls = row![
            checkbox(s.t("list.show_info", &[]), self.show_info)
                .on_toggle(Message::ToggleInfo)
                .size(14)
                .text_size(11),
            text_input(&s.t("list.search", &[]), &self.search)
                .on_input(Message::Search)
                .size(11)
                .padding(4)
                .width(Length::Fixed(160.0)),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        let mut list = column![].spacing(2);
        if visible.is_empty() {
            list = list.push(text(s.t("list.none", &[])).size(12).style(text::secondary));
        }
        let mut last: Option<Severity> = None;
        for f in &visible {
            let sev = f.kind.severity();
            if last != Some(sev) {
                let n = visible.iter().filter(|x| x.kind.severity() == sev).count();
                list = list.push(
                    container(
                        text(format!(
                            "{} — {n}",
                            s.t(&format!("sev.{}", sev.number()), &[]).to_uppercase()
                        ))
                        .size(11)
                        .style(text::secondary),
                    )
                    .padding(iced::Padding {
                        top: 8.0,
                        bottom: 2.0,
                        ..Default::default()
                    }),
                );
                last = Some(sev);
            }
            let selected = self.selected.as_deref() == Some(f.id.as_str());
            let who = f
                .host_name
                .clone()
                .unwrap_or_else(|| f.subject.split(" · ").next().unwrap_or("").to_string());
            let r = row![
                swatch(self.sev_color(sev), 8),
                text(short_host(&who))
                    .size(11)
                    .style(text::secondary)
                    .width(Length::Fixed(76.0)),
                text(plain(&f.title)).size(13).width(Length::Fill),
                text("›").size(13).style(text::secondary),
            ]
            .spacing(10)
            .align_y(Alignment::Center);
            let bg = if selected {
                Some(Background::Color(if self.dark {
                    Color::from_rgb8(0x22, 0x33, 0x55)
                } else {
                    Color::from_rgb8(0xe8, 0xf0, 0xfe)
                }))
            } else {
                None
            };
            let item = container(r)
                .padding([6, 8])
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: bg,
                    border: Border {
                        radius: 6.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                });
            list = list.push(mouse_area(item).on_press(Message::Select(f.id.clone())));
        }
        let hidden = inv
            .findings
            .iter()
            .filter(|f| f.kind.severity() == Severity::Info)
            .count();
        if !self.show_info && hidden > 0 {
            list = list.push(
                container(
                    text(s.t("list.hidden", &[("n", &hidden.to_string())]))
                        .size(11)
                        .style(text::secondary),
                )
                .padding(iced::Padding {
                    top: 8.0,
                    ..Default::default()
                }),
            );
        }
        card(
            column![
                text(s.t("list.title", &[])).size(15),
                text(s.t("list.sub", &[])).size(11).style(text::secondary),
                chips,
                controls,
                scrollable(list).height(Length::Fill)
            ]
            .spacing(6)
            .into(),
        )
        .width(Length::FillPortion(4))
        .height(Length::Fill)
        .into()
    }

    // ---- detail --------------------------------------------------------------------------

    fn view_detail(&self, inv: &Inventory) -> Element<'_, Message> {
        let content: Element<'_, Message> = match self.selected_finding() {
            Some(f) => self.view_finding(inv, f),
            None => self.view_about(inv),
        };
        card(
            scrollable(container(content).padding(iced::Padding {
                right: 14.0,
                ..Default::default()
            }))
            .height(Length::Fill)
            .into(),
        )
        .width(Length::FillPortion(4))
        .height(Length::Fill)
        .into()
    }

    fn view_finding(&self, inv: &Inventory, f: &Finding) -> Element<'_, Message> {
        let s = &self.strings;
        let sev = f.kind.severity();
        let heading = |k: &str| {
            text(s.t(k, &[]).to_uppercase())
                .size(11)
                .style(text::secondary)
        };
        let mut head = row![
            pill(f.kind.label().to_string(), self.sev_color(sev)),
            text(s.t(&format!("sev.{}", sev.number()), &[]))
                .size(12)
                .style(text::secondary)
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if let Some(h) = &f.host_name {
            head = head.push(text("·").size(12).style(text::secondary));
            head = head.push(text(h.clone()).size(12).style(text::secondary));
        }
        let mut col = column![head, text(plain(&f.title)).size(16)].spacing(4);
        col = col.push(Space::with_height(8));
        col = col.push(heading("detail.means"));
        col = col.push(text(s.t(&format!("kind.{}", f.kind.slug()), &[])).size(13));
        col = col.push(Space::with_height(8));
        col = col.push(heading("detail.where"));
        let mut where_col = column![text(plain(&f.detail)).size(12)].spacing(4);
        if let Some(p) = &f.path {
            let ps = p.to_string_lossy();
            if !ps.starts_with("keyring:") && !ps.starts_with("env:") {
                let shown = match f.line {
                    Some(l) => format!("{}:{l}", display_home(p, &inv.home)),
                    None => display_home(p, &inv.home),
                };
                let target = p.display().to_string();
                where_col = where_col.push(text(shown).size(12).font(iced::Font::MONOSPACE));
                where_col = where_col.push(
                    row![
                        small_button(
                            s.t("detail.open_file", &[]),
                            Message::OpenFile(target.clone())
                        ),
                        small_button(s.t("detail.copy_path", &[]), Message::Copy(target))
                    ]
                    .spacing(6),
                );
            }
        }
        col = col.push(inner(where_col.into()));
        col = col.push(Space::with_height(8));
        col = col.push(heading("detail.do"));
        if let Some(h) = &f.hint {
            col = col.push(
                row![
                    text("→").size(13).style(text::secondary),
                    text(plain(h)).size(13)
                ]
                .spacing(8),
            );
        }
        if let Some(c) = &f.command {
            col = col.push(
                row![
                    text("→").size(13).style(text::secondary),
                    column![
                        text(s.t("detail.check_in", &[])).size(13),
                        text(c.clone()).size(12).font(iced::Font::MONOSPACE),
                        small_button(s.t("detail.copy_command", &[]), Message::Copy(c.clone()))
                    ]
                    .spacing(3)
                ]
                .spacing(8),
            );
        }
        if let Some(u) = &f.url {
            col = col.push(
                row![
                    text("→").size(13).style(text::secondary),
                    small_button(
                        s.t("detail.provider_page", &[]),
                        Message::OpenUrl(u.clone())
                    ),
                    text(shorten(u, 48)).size(11).style(text::secondary)
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        if let Some(u) = &f.source_url {
            col = col.push(Space::with_height(8));
            col = col.push(heading("detail.source"));
            let kind = if u.contains("github.com") && u.contains("/blob/") {
                s.t("source.code", &[])
            } else if u.contains("/issues/")
                || u.contains("forum.")
                || u.contains("npmjs.com")
                || u.contains("pypi.org")
            {
                s.t("source.observed", &[])
            } else {
                s.t("source.docs", &[])
            };
            col = col.push(
                column![
                    text(kind).size(12).style(text::secondary),
                    text(shorten(u, 56)).size(11).font(iced::Font::MONOSPACE),
                    row![
                        small_button(s.t("detail.open_link", &[]), Message::OpenUrl(u.clone())),
                        small_button(s.t("detail.copy_path", &[]), Message::Copy(u.clone()))
                    ]
                    .spacing(6),
                ]
                .spacing(3),
            );
        }
        col = col.push(Space::with_height(16));
        col = col.push(
            text(s.t("detail.promise", &[]))
                .size(11)
                .style(text::secondary),
        );
        col.into()
    }

    fn view_about(&self, inv: &Inventory) -> Element<'_, Message> {
        let s = &self.strings;
        column![
            text(s.t("about.title", &[])).size(16),
            Space::with_height(8),
            text(s.t("about.body", &[])).size(13),
            Space::with_height(8),
            text(s.t(
                "about.prices",
                &[
                    ("source", &inv.prices.name),
                    ("as_of", &inv.prices.as_of),
                    ("license", &inv.prices.license)
                ]
            ))
            .size(11)
            .style(text::secondary),
            Space::with_height(8),
            text(s.t("about.pick", &[])).size(12).style(text::secondary),
        ]
        .spacing(4)
        .into()
    }
}

fn chip<'a>(label: String, on: bool, msg: Message) -> Element<'a, Message> {
    let b = button(text(label).size(11)).padding([2, 8]).on_press(msg);
    if on {
        b.style(button::primary).into()
    } else {
        b.style(button::secondary).into()
    }
}

/// `--select <finding id>` (the tower), `--home <dir>` (a fixture), `--since <days>`,
/// `--tab wiring|keys|meter`; other arguments are project roots.
fn parse_args() -> (
    Option<String>,
    Vec<PathBuf>,
    Option<PathBuf>,
    Option<i64>,
    Tab,
) {
    let mut selected = None;
    let mut roots = Vec::new();
    let mut home = None;
    let mut since = None;
    let mut tab = Tab::Wiring;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--select" {
            selected = args.next();
        } else if let Some(v) = a.strip_prefix("--select=") {
            selected = Some(v.to_string());
        } else if a == "--home" {
            home = args.next().map(PathBuf::from);
        } else if a == "--since" {
            since = args.next().and_then(|v| v.parse().ok());
        } else if a == "--tab" {
            tab = match args.next().as_deref() {
                Some("keys") => Tab::Keys,
                Some("meter") => Tab::Meter,
                _ => Tab::Wiring,
            };
        } else if !a.starts_with('-') {
            roots.push(PathBuf::from(a));
        }
    }
    (selected, roots, home, since, tab)
}

/// The scan on a thread, so the window stays responsive.
fn load(home: PathBuf, roots: Vec<PathBuf>, days: i64) -> Task<Message> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let mut env = bbl_hostrules::env::Env::current();
        if env.home != home {
            env = bbl_hostrules::env::Env::fixture(&home, env.os);
        }
        let roots: Vec<PathBuf> = roots
            .iter()
            .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
            .collect();
        if !roots.is_empty() {
            env = env.with_roots(roots);
        }
        let opt = billfold_core::Options {
            since_days: days,
            ..Default::default()
        };
        let inv = billfold_core::scan_with(
            &env,
            &billfold_core::sheets::embedded(),
            &billfold_core::providers::embedded(),
            billfold_core::prices::PriceTable::embedded(),
            &opt,
        );
        let _ = tx.send((Arc::new(inv), started.elapsed().as_millis() as u64));
    });
    Task::perform(
        async move {
            rx.await
                .unwrap_or_else(|_| (Arc::new(Inventory::default()), 0))
        },
        |(inv, ms)| Message::Loaded(inv, ms),
    )
}

/// Titles and details carry Markdown backticks for the CLI; the app shows them plain.
fn plain(s: &str) -> String {
    s.replace('`', "")
}

fn short_host(name: &str) -> String {
    name.split(" (")
        .next()
        .unwrap_or(name)
        .replace(" CLI", "")
        .replace("GitHub ", "")
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use billfold_core::Kind;

    #[test]
    fn locales_have_the_same_keys() {
        bbl_ui::i18n::same_keys(EN, FR).unwrap();
    }

    #[test]
    fn every_kind_and_severity_has_its_strings() {
        for lang in [Lang::En, Lang::Fr] {
            let s = Strings::load(EN, FR, lang);
            for k in Kind::ALL {
                assert_ne!(
                    s.t(&format!("kind.{}", k.slug()), &[]),
                    format!("kind.{}", k.slug()),
                    "missing kind.{}",
                    k.slug()
                );
            }
            for sev in Severity::ALL {
                assert_ne!(
                    s.t(&format!("sev.{}", sev.number()), &[]),
                    format!("sev.{}", sev.number())
                );
            }
        }
    }

    #[test]
    fn helpers() {
        assert_eq!(plain("a `b` c"), "a b c");
        assert_eq!(short_host("Codex CLI"), "Codex");
        assert_eq!(short_host("GitHub Copilot"), "Copilot");
        assert_eq!(shorten("abcdef", 4), "abc…");
    }
}
