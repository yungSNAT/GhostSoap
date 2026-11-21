// Copyright 2026 Ryan Duguid
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL-3.0 section 7(b): see NOTICE.

//! Register entries and packs: the model and its validation. Pure functions, no I/O.

use std::collections::{BTreeMap, BTreeSet};

use jiff::ToSpan;
use jiff::civil::Date;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use toml::value::Datetime;

mod validate;

pub use validate::*;

/// What an entity does with personal information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    CreditReportingBody,
    PropertyData,
    PeopleSearch,
    MarketingList,
    AdTech,
    Directory,
    Loyalty,
    Recruitment,
    Telecommunications,
    FinancialServices,
    Insurance,
    Retail,
    OnlineService,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Priority {
    Crucial,
    High,
    Standard,
}

/// How far an entry has been checked (plan section 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Quality {
    /// A GhostSoap request got a reply.
    Tested,
    /// The contact was checked against the entity's own pages, by a person or an AI agent,
    /// with the evidence recorded in `docs/provenance.md`.
    Verified,
    /// Taken from a licence-compatible dataset.
    Imported,
    /// Named in a non-reusable source and not yet checked.
    Lead,
}

impl Quality {
    /// Only tested and verified entries may enter packs.
    pub fn is_packable(self) -> bool {
        matches!(self, Self::Tested | Self::Verified)
    }
}

/// A request the law lets the user make (plan section 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Lever {
    App7NoMarketing,
    App7NoFacilitation,
    App7Source,
    App11Destroy,
    App12,
    App13,
    App13NotifyRecipients,
    App13Statement,
    S20rAccess,
    S20tCorrection,
    S20kBan,
    S21tAccess,
    S21vCorrection,
    SpamUnsubscribe,
}

impl Lever {
    pub const ALL: [Lever; 14] = [
        Self::App7NoMarketing,
        Self::App7NoFacilitation,
        Self::App7Source,
        Self::App11Destroy,
        Self::App12,
        Self::App13,
        Self::App13NotifyRecipients,
        Self::App13Statement,
        Self::S20rAccess,
        Self::S20tCorrection,
        Self::S20kBan,
        Self::S21tAccess,
        Self::S21vCorrection,
        Self::SpamUnsubscribe,
    ];

    /// The lever named `name` in register files.
    pub fn from_name(name: &str) -> Option<Lever> {
        Self::ALL.into_iter().find(|l| l.name() == name)
    }

    /// The name used in register files.
    pub fn name(self) -> &'static str {
        match self {
            Self::App7NoMarketing => "app7-no-marketing",
            Self::App7NoFacilitation => "app7-no-facilitation",
            Self::App7Source => "app7-source",
            Self::App11Destroy => "app11-destroy",
            Self::App12 => "app12",
            Self::App13 => "app13",
            Self::App13NotifyRecipients => "app13-notify-recipients",
            Self::App13Statement => "app13-statement",
            Self::S20rAccess => "s20r-access",
            Self::S20tCorrection => "s20t-correction",
            Self::S20kBan => "s20k-ban",
            Self::S21tAccess => "s21t-access",
            Self::S21vCorrection => "s21v-correction",
            Self::SpamUnsubscribe => "spam-unsubscribe",
        }
    }
}

/// Identity details a letter may include. Document numbers are deliberately absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityField {
    FullName,
    AliasNames,
    Email,
    Phone,
    DateOfBirth,
    Address,
    PreviousAddresses,
    Postcode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExemptionRisk {
    None,
    SmallBusinessPossible,
    Overseas,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Escalation {
    Oaic,
    Afca,
    Tio,
    Acma,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Effort {
    Easy,
    Medium,
    Hard,
    Impossible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChannelKind {
    Email,
    WebForm,
    Post,
    Phone,
}

impl ChannelKind {
    /// The channel in words, for messages to the user.
    pub fn label(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::WebForm => "web form",
            Self::Post => "post",
            Self::Phone => "phone",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    pub kind: ChannelKind,
    /// An email address, form URL, postal address or phone number, by `kind`.
    pub address: String,
    /// The entry's levers this channel can carry; empty for a contact-only channel.
    pub levers: Vec<Lever>,
    /// The channel to use for each of its levers; at most one per lever.
    #[serde(default)]
    pub preferred: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualCheck {
    /// The entity's own search or opt-out page; GhostSoap only opens it.
    pub url: String,
}

/// Why an entry was retired. The reasons follow datenanfragen's obsolete records (CC0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObsoleteReason {
    /// Bought by another entity, which now handles requests.
    Acquired,
    /// Merged into another entity.
    Merged,
    /// The same entity as another entry.
    Duplicate,
    /// Closed, with no successor found.
    Closed,
    /// Should not have been added: it does not meet the inclusion criteria.
    OutOfScope,
}

impl ObsoleteReason {
    fn name(self) -> &'static str {
        match self {
            Self::Acquired => "acquired",
            Self::Merged => "merged",
            Self::Duplicate => "duplicate",
            Self::Closed => "closed",
            Self::OutOfScope => "out-of-scope",
        }
    }

    /// Whether requests now belong to another entry, which must be named.
    fn needs_redirect(self) -> bool {
        matches!(self, Self::Acquired | Self::Merged | Self::Duplicate)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Obsolete {
    pub reason: ObsoleteReason,
    pub replaced_by: Option<String>,
}

impl Obsolete {
    /// Why a plan skips this entry, and where to go instead.
    pub fn skip_reason(&self) -> String {
        match &self.replaced_by {
            Some(id) => format!("obsolete ({}); use {id} instead", self.reason.name()),
            None => format!("obsolete ({})", self.reason.name()),
        }
    }
}

/// One entity in `registry/au/<id>.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Permanent slug, equal to the file name; never reused.
    pub id: String,
    pub name: String,
    pub legal_entity: String,
    /// Required, except on an overseas entry, which has none (plan section 6).
    pub abn: Option<String>,
    pub kinds: Vec<Kind>,
    pub priority: Priority,
    pub quality: Quality,
    pub verified_at: Datetime,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub levers: Vec<Lever>,
    /// Levers the user must choose per entity; never part of a pack (plan section 6a).
    #[serde(default)]
    pub opt_in_levers: Vec<Lever>,
    pub identity_fields: Vec<IdentityField>,
    pub exemption_risk: ExemptionRisk,
    /// Ordered escalation targets.
    pub escalation: Vec<Escalation>,
    pub effort: Effort,
    #[serde(default)]
    pub channels: Vec<Channel>,
    pub manual_check: Option<ManualCheck>,
    pub obsolete: Option<Obsolete>,
    /// Hashes of source pages at verification, for the monthly drift check.
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

impl Entry {
    /// A site whose responsible entity is overseas with no ABN: a people-search site with no
    /// Australian presence, or a service whose policy names an overseas company. It gets a
    /// manual check and its own opt-out form, never a letter.
    pub fn is_overseas(&self) -> bool {
        self.exemption_risk == ExemptionRisk::Overseas
    }
}

/// A source page's state when the entry was verified (normalisation in `registry/README.md`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    /// One of the entry's `sources`.
    pub url: String,
    pub checked_at: Datetime,
    /// Lowercase hex SHA-256 of the page's normalised main text.
    pub sha256: String,
}

/// A named group of entries in `registry/au/packs/<id>.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub id: String,
    pub name: String,
    pub description: String,
    pub entries: Vec<String>,
}

/// Entry files and pack files, each sorted by name.
#[derive(Debug, Clone)]
pub struct RegisterFiles {
    pub entries: Vec<SourceFile>,
    pub packs: Vec<SourceFile>,
}

/// A register file: its file name and contents.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub name: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    /// The file name the finding is about.
    pub file: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct Report {
    /// Entries and packs that parsed.
    pub entries: usize,
    pub packs: usize,
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.count(Severity::Error)
    }

    pub fn warnings(&self) -> usize {
        self.count(Severity::Warning)
    }

    fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == severity)
            .count()
    }

    fn push(&mut self, severity: Severity, file: &str, message: String) {
        self.findings.push(Finding {
            severity,
            file: file.to_owned(),
            message,
        });
    }
}

/// Parsed entries and packs, keyed by id.
#[derive(Debug)]
pub struct Register {
    pub entries: BTreeMap<String, Entry>,
    pub packs: BTreeMap<String, Pack>,
}

/// Parse register files without validating them (`check` does that, and CI runs it on the
/// built-in register). Fails on the first file that does not parse.
pub fn parse_register(files: &RegisterFiles) -> Result<Register, String> {
    let mut register = Register {
        entries: BTreeMap::new(),
        packs: BTreeMap::new(),
    };
    for file in &files.entries {
        let entry: Entry = toml::from_str(&file.text).map_err(|e| format!("{}: {e}", file.name))?;
        register.entries.insert(entry.id.clone(), entry);
    }
    for file in &files.packs {
        let pack: Pack =
            toml::from_str(&file.text).map_err(|e| format!("packs/{}: {e}", file.name))?;
        register.packs.insert(pack.id.clone(), pack);
    }
    Ok(register)
}

#[cfg(test)]
mod tests;
