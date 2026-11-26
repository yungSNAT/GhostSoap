# GhostSoap repository plan

_GhostSoap at no Price._

Draft 2, 24 October 2026, amended since. Status: approved (section 12); M0 to M2, M3a and M4 complete (including `scan`, `demo`, `audit` and `export --escalation`); public since 24 October 2026; M3 split into M3a and M3b on 9 November 2026 after the 29 October review (section 14); v0.1 to ship without M3b (decided 10 November 2026, section 12).

GhostSoap is a free, local-first Rust tool that helps people in Australia reclaim their privacy. It prepares Australian Privacy Act requests to data brokers and other businesses for the user to send from their own mailbox, tracks every deadline and reply, builds an escalation file for the Office of the Australian Information Commissioner (OAIC), and walks the user through the wider data hygiene steps that email cannot fix.

## 1. Why another tool

Research on 24 October 2026 (sources at the end of this plan) found:

- **Few services use Australian rights.** The 24 October research found none, and only one comparable paid service had an Australian list (55 brokers, about 40 of them US business-data or ad-tech firms; from A$49 a year); other major paid services either listed Australia under US or EU law or excluded Australia. By 5 November 2026 a comparable managed Australian service advertised requests under the Privacy Act (A$129 to A$259 for a year of re-scanning), and a comparable provider published a free guide citing the APPs.
- **Few open source tools cover Australia.** On 24 October none did; comparable open source projects focused on the US or EU. On 8 November 2026 a comparable UK-first open source tool added an Australian letter combining APP 7.6 and APP 11.2 requests (marked as not yet used for real), without access (APP 12), correction (APP 13), credit reporting (Part IIIA) or Spam Act requests. Read on 13 November 2026, that comparable tool also sent from the user's mailbox, read replies and drafted a follow-up complaint to the business, a reply to a demand for ID and an OAIC complaint that skipped any external dispute resolution step, whilst none of its default broker entries was tagged with Australian law.
- **No licence-compatible Australian broker dataset exists.** Existing public broker directories are CC BY-NC-SA, so they can serve only as leads.
- **The category underperforms.** Consumer Reports (August 2024, 32 volunteers) found paid services removed 35% of people-search profiles within 4 months; manual opt-outs did better.

GhostSoap's edge is to be honest about Australian law, use the levers that law actually provides, keep the user's data on the user's machine, and keep a small, sourced and verified register.

## 2. Principles and lines not to cross

1. **The user acts; GhostSoap prepares.** Every request goes out from the user's own mailbox, in the user's name, after the user has reviewed it. GhostSoap does not act as an agent or representative.
2. **Local first.** No server, account, telemetry or analytics. The only network traffic is what the user starts: sending mail, reading replies, opening a link.
3. **Minimum data.** Each register entry lists the identity fields that entity needs; a letter includes only those. GhostSoap never stores identity document images or Medicare, passport or licence numbers.
4. **No evasion.** No scraping of people-search sites, CAPTCHA solving, IP rotation or automated form filling. GhostSoap opens the broker's own search or opt-out page for the user and records what they found.
5. **Honest outcomes.** Results read 'requested', 'confirmed' or 'refused', never 'deleted' unless the entity confirms deletion. An honesty panel states what Australian law cannot force.
6. **Requests about the user only.** No requests for another person; household use means a separate profile per consenting adult.
7. **Information, not legal advice.** GhostSoap supplies templates and guidance and points to the OAIC, IDCARE and community legal centres. It never makes statutory declarations or Australian Electoral Commission (AEC) applications for the user.
8. **No conflicts.** No affiliate links, broker partnerships or paid tiers.

## 3. Legal model

Australia has no general right to erasure today. The tranche 2 Privacy Act reform is an Exposure Draft, not law (submissions closed 18 September 2026), and its erasure right would reach large digital platforms only (see 'Law status' below). GhostSoap therefore uses these levers, each stored in a law table with its source and deadline:

Every row below was read in the primary text on 25 October 2026: the Privacy Act 1988 compilation C2026C00227 (in force from 4 June 2026; versions start 10 December 2026 and 1 January 2027, and the monthly evidence check reports each once the Federal Register records it as latest), the Spam Act 2003 compilation C2016C00614, the Spam Regulations 2021 (F2021L00285), the Criminal Code compilation C2026C00243 and the OAIC's APP guidelines (paragraph numbers given). The law table in `src/core/law.rs` holds the same values with their citations, and `ghostsoap law` prints it.

| Lever | Request | Deadline basis |
|---|---|---|
| APP 7.6(c), 7.7(a) | Stop sending me direct marketing; free | Reasonable period, OAIC: generally no more than 30 days (para 7.37) |
| APP 7.6(d), 7.7(a) | Stop using or disclosing my information to facilitate others' direct marketing, including by sale; free | Reasonable period, OAIC: no more than 30 days (para 7.41, without 'generally'). Selling data to another organisation is the OAIC's own example of facilitating (paras 7.40, 7.43), so APP 7 reaches brokers that only sell |
| APP 7.6(e), 7.7(b) | Tell me the source of my information; free | Generally 30 days (para 7.47), unless impracticable or unreasonable; the organisation must justify that (para 7.46); source tracing finds the next broker upstream |
| APP 12 | Give me access to the information you hold | Organisations: reasonable period (APP 12.4(a)(ii)), OAIC: should not exceed 30 calendar days (para 12.67); agencies: 30 days. No charge for the request; a non-excessive charge for giving access is allowed (APP 12.8, paras 12.77–12.81), so the letter asks for any charge to be stated first. 10 refusal grounds (APP 12.3); a refusal needs written reasons and complaint options (APP 12.9) |
| APP 13 | Correct information that is inaccurate, out of date, incomplete, irrelevant or misleading; free (APP 13.5(b)) | Reasonable period, OAIC: should not exceed 30 calendar days (para 13.63) |
| APP 13.2 | Tell the entities you disclosed my information to about the correction | No time set: reasonable steps unless impracticable or unlawful |
| APP 13.4 | Attach my statement where you refuse to correct | As APP 13 (APP 13.5 covers it; para 13.63) |
| APP 11.2 | Destroy or de-identify information you no longer need | An entity obligation, not an individual right; asked for, never demanded |
| s 20R | Credit reporting body access | Within a reasonable period, not longer than 10 days (s 20R(3)); free if no request in the previous 3 months (s 20R(5)) |
| s 20T | Credit reporting body correction | Within 30 days or a longer period the individual agrees to in writing (s 20T(2)); free (s 20T(5)) |
| s 21T, s 21V | Credit provider access; credit provider correction | Access: reasonable period (s 21T(3)); correction: 30 days (s 21V(2)) |
| s 20K | Ban period after suspected fraud | Starts on the request and ends 21 days later, extendable on request, more than once (s 20K(3)–(5)); free (s 20K(6)) |
| Spam Act 2003 | Unsubscribe | Withdrawal of consent takes effect at the end of 5 business days from the day an electronic unsubscribe message is sent (from service for post, from delivery otherwise), with public holidays where the message was sent to (Sch 2 cl 6); only email channels may serve this lever, so letters state the sending rule. The unsubscribe address must be reasonably likely to receive unsubscribe messages for at least 30 days after the message is sent (s 18(1)(e)) and must not need a fee, an account or personal information beyond the address (Spam Regulations 2021 s 7) |
| s 40(1A)–(1B), s 41 | Escalation | Complain to the entity first; the Act sets no waiting period, and OAIC guidance allows about 30 days (para 12.87). Then the entity's external dispute resolution scheme if it has one (AFCA for credit entities, the TIO for telcos), then the OAIC, within 12 months of becoming aware (the OAIC may decline later complaints, s 41(1)(c)). The OAIC has a discretion not to investigate (s 41(1)(da), (dc), (dd)) and publishes a long wait (see 'Law status' below). The complain-first precondition does not apply to complaints about ss 20R, 20T, 21T or 21V. Spam complaints go to ACMA through its spam complaint form with a copy of the message; telemarketing complaints go to donotcall.gov.au. Each register entry lists its escalation targets |

APP 12 and 13 set no form for a request: an entity may publish a preferred procedure but 'cannot require an individual to follow a particular procedure, use a designated form or explain the reason for making the request' (paras 12.18, 12.21, 13.20, 13.22). A letter to any published privacy address is therefore a valid request.

APP 7 applies only where the entity uses or discloses information for direct marketing, and is displaced to the extent the Interactive Gambling Act 2001 (Pt 7B Div 5), the Do Not Call Register Act 2006 or the Spam Act applies (APP 7.8). A broker that does neither gets APP 12 and 13 letters, not APP 7, and a reply to that effect is recorded as `not-direct-marketing`.

The Privacy and Other Legislation Amendment Act 2024 added a statutory tort for serious invasions of privacy (Privacy Act Schedule 2, s 94A; commenced 10 June 2025) and doxxing offences (Criminal Code ss 474.17C and 474.17D; commenced 11 December 2024). GhostSoap prepares neither claims nor reports; the honesty panel and checklist point to a community legal centre, police and eSafety.

Each register entry records an **exemption risk**: `none`, `small-business-possible` (turnover under A$3 million, unless it discloses personal information for a benefit, service or advantage, or pays to collect it, under s 6D(4)(c)–(d); s 6D(7)–(8) narrow both carve-outs), or `overseas` (responsible entity overseas with no ABN found: no levers, a manual check only; the Act can still reach an overseas organisation that carries on business in Australia (s 5B(1A), (3)), so GhostSoap says it has not established a legal basis rather than that none exists). The letter text and the honesty panel change with it.

A future `au-erasure` letter waits for passed legislation. Adding it needs a `Lever` variant, a law table row and a letter template; the compiler rejects a lever missing either.

### Law status, read 5 November 2026

This panel records what the primary sources said on that date, or on the date a row gives; `ghostsoap law` prints its short form. Sources are in the list at the end of this plan. Re-read the lever provisions when the 10 December 2026 compilation registers, because nothing below has been checked against that text.

| When | Source | What it does | Effect on GhostSoap |
|---|---|---|---|
| In force | Privacy Act 1988, C2026C00227 | The Federal Register shows it as the latest version, ending 10 December 2026; no later compilation has a register ID | Every lever above rests on this text |
| 10 December 2026 | Privacy and Other Legislation Amendment Act 2024, sch 1 items 87 and 88 | Insert APP 1.7 to 1.9 and add APP 1.7 to s 13K(1)(b). Item 89 applies them to decisions made after commencement | Transparency duty only; see 'Automated decisions' below |
| 1 January 2027 | Interactive Gambling Amendment (Gambling Reform) Act 2026, sch 1 items 46 and 47 | Add s 33C(1)(h) and insert 'and subsection 62P(2)' in APP 7.8(aa) | The APP 7.8 note in this section needs rereading; no lever changes on the face of the items |
| By 10 December 2026 | Privacy Act s 26GC, inserted by the 2024 Act (assented to 10 December 2024); read 6 November 2026 | Section 26GC(10): 'The Commissioner must develop and register the Children's Online Privacy Code within the period of 24 months beginning on the day the Privacy and Other Legislation Amendment Act 2024 receives the Royal Assent.' The OAIC says 'The final Code must be registered by 10 December 2026'. It consulted on an Exposure Draft from 31 March to 5 June 2026, and the Federal Register had no such title on 6 November 2026 | No lever uses it. It would bind APP entities within s 26GC(5), so read the final code for any change to the APP levers when it registers |
| Not law | Exposure Draft, Privacy Amendment (Personal Data Protection) Bill 2026 | Released with a consultation paper; consultation opened 31 August and closed 18 September 2026. The Attorney-General's Department says the Bill 'remains subject to further consideration by government'. The draft's commencement table is blank. At the 31 August press conference the Attorney-General spoke of 'the introduction of the Bill this year' | No lever or letter relies on it; see 'Tranche 2 levers to watch' below |

**Automated decisions.** From 10 December 2026, APP 1.7 to 1.9 require an APP privacy policy to state the kinds of personal information used in certain computer programs and the kinds of decisions those programs make or substantially and directly support, where a decision could reasonably be expected to significantly affect an individual's rights or interests. The OAIC's September 2026 fact sheet and APP 1 guidelines (version 2) say a decision can be in scope even where a person reviews the program's output. This is a transparency duty on the entity. It gives the user no request to make, so it gets no lever and no register field. The monthly evidence hashes already flag a changed privacy policy where that page is tracked.

**The OAIC's published wait.** The OAIC's blog of 2 March 2026 says: 'As at February 2026, it is unlikely that we will be able to substantially progress new validly lodged individual privacy complaints for some 6 to 12 months after they are lodged (unless we determine there are exceptional circumstances which may warrant expeditious consideration).' It also says 'Not all individual privacy complaints will be taken through to investigation', that a person should generally give the entity 30 days, and that a person should generally try an approved external dispute resolution scheme first, listing telecommunications providers among the areas covered. The OAIC's register lists 10 recognised schemes, including AFCA and the TIO. One search of the OAIC's site found no later statement of the wait; the February figure may have changed.

**Tranche 2 levers to watch.** Each row describes the Exposure Draft or the consultation paper, neither of which is law. The draft may change before introduction, and every effect is conditional on enactment as drafted.

| Lever or field | What the draft proposes | What GhostSoap would then need |
|---|---|---|
| `app7-no-marketing` | Draft APP 7.2: on a request, an organisation 'must take such steps as are reasonable in the circumstances to give effect to the request'. The draft clause has no 'reasonable period' and no bar on charging, both of which current APP 7.7 has | New deadline and charge text |
| `app7-no-facilitation`, `app7-source` | Item 27 repeals clause 7 and substitutes draft APP 7.1 to 7.7, which have no counterpart to current APP 7.6(d) or (e). Draft APP 4.2 instead says an organisation 'must not trade the information unless the individual has consented to the trading of the information', subject to exceptions; draft s 6FC makes a disclosure a trade if it is for money or other consideration or for the purposes of direct marketing, with carve-outs | Both letters would lose their present basis; a replacement would need its own reading of draft APP 4 |
| `app11-destroy` | Draft APP 11.2 adds a duty to 'consider whether to destroy the personal information' before destroying or de-identifying it. It stays a duty on the entity | Reword the letter; still asked for, not demanded |
| `app12` | Draft Schedule 4 Part 1 adds an exception for technical impossibility or infeasibility. The consultation paper says the existing timeframes for responding remain unchanged | Reply handling for the new ground |
| `au-erasure` (not built) | Draft APP 14.1: 'If an organisation that is a large digital platform holds personal information that relates to an individual, the organisation must, on request by the individual, destroy the information.' Draft s 6EB limits this to providers of services defined in the Online Safety Act 2021 that meet a gross revenue test of at least $500 million or an end users test of at least 2.5 million, or are prescribed. Draft APP 14.2 and 14.3 give exceptions | A new lever for large digital platforms only, and only after the law passes |
| `exemption_risk` | Draft s 6D(4)(c) and (d) restate the small business carve-outs in terms of trading | Recheck `small-business-possible` |
| `escalation` | The consultation paper, with no draft provision, proposes that APP entities 'respond within 60 days' to complaints and that individuals generally raise concerns with the entity first | Recheck the 30-day guidance in this section |

## 4. Scope

**In v0.1 (M0 to M2, M3a and M4):** register, letters, draft-only sending (`.eml` files, `mailto:` links and `--copy`), `scan` (opening people-search pages), zero-PII `demo` (text and HTML), encrypted local tracker of every message and request with dated deadlines and a due list, hash-chained `audit` log, `export` (with SHA-256 manifests and `--escalation` OAIC bundles), `forget`, Australian hygiene checklist, shell completions, CLI.

**M3b, after v0.1:** replies to pushback, reminders that escalate, the complaint letter, re-check reminders, outcome reporting, the new register fields and the backlog. As the 29 October review recommended, v0.1 ships without M3b (section 12). Until M3b ships, follow-up letters and formal complaints remain manual guidance (`export --escalation` packages the tracked record and audit excerpt with state and Privacy Act s 40(1A) notices).

**Later (M5 to M7):** SMTP sending, read-only IMAP reply queue, an interface for non-technical users.

**Out of scope:** non-Australian brokers (the user can use other tools; GhostSoap may link to them), except overseas people-search sites that list Australians and, since 13 November 2026, overseas business contact-data brokers, which get manual opt-out entries only (section 6), account creation on brokers, automated web forms, people-search scanning, any hosted service.

## 5. Architecture

One crate, `ghostsoap`, with a library and a thin `src/main.rs` (house convention: standalone crate, `[lib]` split). Edition 2024, `rust-version = "1.99"`, `rust-toolchain.toml` pinned to `1.99.1`, release profile `lto = true`, `codegen-units = 1`.

The library keeps two layers apart:

- **`core` (no I/O):** register model and validation, law table, letter rendering, request state machine, deadline calculation, checklist model. Pure functions over values, so the same code can later compile to WebAssembly (M7) without a rewrite.
- **`io`:** files, encryption, mail client hand-off, SMTP and IMAP (later), terminal prompts.

### Data model

**Register entry**, one TOML file per entity under `registry/au/`:

```toml
id = "example-broker-au"          # permanent slug; never reused
name = "Example Broker"
legal_entity = "Example Broker Pty Ltd"
abn = "00 000 000 000"            # checked against ABN Lookup
kinds = ["marketing-list"]        # credit-reporting-body, property-data, people-search, marketing-list, ad-tech, directory, loyalty, recruitment, telecommunications, financial-services, insurance, retail, online-service
priority = "high"                 # crucial | high | standard
quality = "verified"              # tested | verified | imported | lead
verified_at = 2026-10-24
sources = ["https://example.com.au/privacy"]
levers = ["app7-no-marketing", "app7-no-facilitation", "app7-source", "app13", "app13-notify-recipients", "app13-statement"]
opt_in_levers = ["app12"]         # never in a default pack; see section 6a
identity_fields = ["full_name", "email", "postcode"]
exemption_risk = "none"
escalation = ["oaic"]             # ordered; for example ["afca", "oaic"] for a credit entity, ["tio", "oaic"] for a telco
effort = "easy"                   # easy | medium | hard | impossible

[[channels]]
kind = "email"                    # email | web-form | post | phone
address = "privacy@example.com.au"

[manual_check]
url = "https://example.com.au/search"   # the broker's own page; GhostSoap only opens it

# obsolete = { reason = "merged", replaced_by = "other-id" }
```

Quality grades follow a comparable open source register convention: `tested` means a GhostSoap request got a reply; `verified` means the contact was checked against the entity's own privacy policy or contact pages, by a person or an AI agent, with the evidence recorded in `docs/provenance.md`; `imported` means taken from a licence-compatible dataset; `lead` means named in a non-reusable source and not yet checked. Only `tested` and `verified` entries enter packs by default.

Planned register fields (M3b, not yet in the schema), following comparable register flags: `last_tested` (the date of the latest `tested` reply, beside `verified_at`), `needs_id_document`, `needs_phone` and `paid_removal`. `plan` and `draft` warn before a letter goes to an entry carrying one of these flags.

**Packs** (`registry/au/packs/*.toml`) group entries: `credit` (Equifax, Experian, which now includes illion, and any other credit reporting body), `property`, `marketing`, `people-search`, `loyalty`, plus `start-here` for users with little time.

**Message and request**, held in the tracker (M3a). A message is one draft as 'Letters' below describes it: its tag, entry id, channel (kind and address), the folder and name of its draft file, drafted date and its records of sending: the date and channel the user actually used, or a withdrawal of that record. The last record decides whether the message counts as sent. It holds one request per lever, each with its own letter version, the deadline kind the law table gave when it was drafted and an append-only event list, so a reply that names the source but does not stop the marketing is recorded lever by lever. An event is a status with its date and an optional reason, source and note; a longer period agreed in writing; or a withdrawal, which voids the latest status or longer period still in effect. A request's status is never stored: it is the latest status event in effect, or `sent` or `drafted` from the message. A mistake is put right by a withdrawal record, never by editing or deleting what was recorded. Request statuses: `drafted`, `sent`, `acknowledged`, `id-requested`, `charge-quoted`, `fulfilled`, `partial`, `suppressed` (the entity says it stopped using or disclosing the information, not that it destroyed it), `no-record`, `refused-with-reasons`, `refused`, `exempt-claimed`, `bounced`, `relisted`, `escalated`, `closed` and `abandoned`. The last 2 are the closure reasons: `closed` is finished with an answer, `abandoned` is given up without one. Reasons, recorded only with a refusal or a claimed exemption: `not-direct-marketing`, `small-business`, `outside-jurisdiction`, `insufficient-verification` and `other`; the last 3 names follow the Data Rights Protocol (`docs/provenance.md`). The transition model lists every allowed change of status and refuses any other; after `closed` only `relisted` can be recorded, and after `abandoned` nothing, unless the closing status is withdrawn. Separately, every load and save checks the rules that hold whatever the history (valid, unique tags; one request per lever; each request's deadline of the kind its lever's law gives; no event dated before the message was sent; no answer on a message not recorded as sent, `abandoned` excepted; a reason only with a refusal or a claimed exemption; a withdrawal only of something recorded) and refuses a state that breaks one. The transition table is never replayed on load, so a later version can change it without making an older file unreadable. The APP 13.2 request has no deadline and becomes actionable only after a recorded correction (OAIC para 13.49); the APP 13.4 period starts at a recorded refusal (para 13.59), not on the send date.

`overdue` is never stored: `status` derives it from the deadline and the date. `ghostsoap status` groups requests into 5 action states, each request in the first that applies: overdue (`sent` or `acknowledged`, with its statutory due date or OAIC guide date passed; `status` says which), action needed (`drafted`, `id-requested`, `charge-quoted`, `partial`, `refused-with-reasons`, `refused`, `exempt-claimed`, `bounced`, `relisted`, and `suppressed` on a request that asked for more than marketing to stop, since the user must send, answer, complain or close), waiting (`sent`, `acknowledged`, `escalated`), done (`fulfilled`, `no-record`, `closed`, and `suppressed` on `app7-no-marketing`, `app7-no-facilitation` or `spam-unsubscribe`, which ask for exactly that) and abandoned (`abandoned`). Overdue items are listed first.

**Deadlines** (M3a) are calculated by pure functions from the recorded send date (APP 13.4 excepted, as above), never the draft date, and an explicit as-of date. Each request keeps its lever's deadline kind from the law table: `Days` gives a statutory due date, moved by a longer period the user records agreeing to in writing (ss 20T(2), 21V(2)); `BusinessDays`, the Spam Act's 5 business days, gives the date withdrawal of consent takes effect, from the public holidays in the place the message was sent to, and is shown as unknown without them; it is not a reply deadline and never makes a request overdue. `Reasonable` with a guide gives an OAIC guide date, which `status` never calls a breach; `Reasonable` without a guide and `NotStated` give no date. The s 20K 21 days is a ban period, not a reply deadline. Statutory dates are counted as the Acts Interpretation Act 1901 s 36 directs (compilation C2026C00117, in force from 28 March 2026, read 16 November 2026); the Privacy Act sets no rule of its own for counting them. The 10 days of s 20R(3) begin after the day the request is made, which is not counted (s 36(1) item 6); the 30 days of ss 20T(2)(a) and 21V(2)(a) start on that day, which is counted (item 2). A last day on a Saturday or Sunday moves to the Monday (s 36(2)). A public holiday in the place the entity acts would move it further, which GhostSoap does not know, so `status` says so.

**Calendar file** (specified 17 November 2026; not in v0.1 unless Ryan adds it, decision 11): `status --ics <file>` also writes an iCalendar file (RFC 5545) to a new path, refusing a file or link already there, mode 600 on Unix. Lines end in CRLF and fold at 75 octets, never inside a character; TEXT values escape backslash, semicolon, comma and newline. The calendar carries `VERSION:2.0`, `PRODID:-//yungSNAT//GhostSoap <version>//EN`, `CALSCALE:GREGORIAN` and `METHOD:PUBLISH`. It holds one all-day event for each request of a message not superseded whose action state is overdue, action needed or waiting and that has a statutory date or an OAIC guide date; a request that is done, abandoned or undated has none, and a file written later does not remove an event an earlier one gave. Each event has `UID:<tag>-<lever>@ghostsoap.invalid`, the same in every file whatever its date; `DTSTAMP`, the time of this export in UTC; `DTSTART;VALUE=DATE` on the date and `DTEND;VALUE=DATE` the day after; `SEQUENCE`, the number of records the message holds (records of sending and every request's events), which only grows and grows with each record that can move the date; a summary of tag, entry, lever and which kind of date; a description of what the date means, with the public-holiday limit for a statutory date; and `TRANSP:TRANSPARENT`. No alarm, attendee or organiser, and no profile field or user's words. The file names the businesses and requests, so `status` warns that importing it into an online calendar shares them with the calendar's provider, and warns when its folder is in a Git working tree or a known sync folder. Stable UIDs let a calendar that honours them update an event on re-import; they do not guarantee it, and some calendar apps add a second copy, which the README says. `forget` does not look for the file.

**Profile**: the user's identity fields, previous addresses and alias names; never document numbers.

### Storage and secrets

- One state file (profile, messages, requests and events) encrypted with the `age` crate under a passphrase (scrypt), entered through a hidden prompt (`rpassword`).
- M2 writes state format 1, which holds the profile alone. M3a adds format 2: GhostSoap reads a format 1 file through an explicit migration that starts with no messages, refuses a format newer than it knows, and leaves the format 1 file unchanged if the migration fails. The migration is made in memory: the first command that changes the state replaces the file as format 2, and a command that changes nothing writes nothing. Every update is one load, change, encrypt and replace under the state lock. A state is checked against the data model's standing rules when it is read and before it is written, and one that would not read back is never saved. Tests use a real M2 state file, a held lock, a wrong passphrase and a future format.
- State lives in the platform data directory (`%LOCALAPPDATA%\GhostSoap` on Windows, the XDG data directory on Linux), never under `Documents` or another folder OneDrive or a similar service may sync.
- Writes go to a temporary file in the same directory, `sync_all`, then rename, retrying a rename that fails because antivirus or an indexer holds the file. The temporary file is made anew for each write, after removing any file or link left at its name, so a write never goes through a link. On Unix the folder is flushed after the rename; if that fails, GhostSoap says the state was saved but a power cut could undo it, never that nothing was saved. A lock file stops 2 processes overwriting each other. After an interruption the state file holds either the previous or the new complete state, never a partial one, and the next command shows which; after an interrupted `init` the user runs it again.
- The OS keyring cache for the key is optional and off by default: it trades the passphrase for OS-login protection, since any process running as the user can read it. SMTP credentials (M5) live only in the keyring.
- Plaintext outside the state file is the weakest point: drafts, exports, the escalation pack and the mailbox's Sent folder. Drafts go to a `drafts` folder in the data directory (private to the user, unlike a shared temporary folder such as Linux's `/tmp`; on Unix the folder is mode 700 and each draft 600) and `ghostsoap clean` removes them. Each draft is written to a temporary file of its own and its SHA-256 is committed to the `.ghostsoap-drafts` manifest before the file is hard-linked into place. Publication never replaces an existing file; the manifest keeps only each name's latest record and is replaced whole by renaming a complete copy over it, never edited in place; a lock file serialises `write` and `clean` between GhostSoap processes. `clean` deletes a committed draft only while it still holds those bytes (a file put at that name with exactly those bytes counts as the draft), and reports, without deleting, a listed file that changed, a pending name whose write never committed, a file named like a draft that the manifest does not list (such as a draft whose record was lost) and temporary files left by an interrupted write or manifest update, on every run until they are dealt with; it refuses a manifest that is a link, keeps a `--out` folder's other files and removes only GhostSoap's own emptied folder, never one named with `--out`. `draft` warns, never refuses, when the drafts folder (checked as given and with links resolved, or at its nearest existing ancestor if not yet made) is inside a Git working tree (a `.git` folder or file at or above it) or a OneDrive, Dropbox or iCloud Drive folder at its usual location; the check is incomplete, so no warning does not make a folder private. On Unix a drafts folder others can open, or a link, is refused; export and escalation warn where they write; the README says plainly that the Sent folder holds copies.
- `ghostsoap export` (M3a) writes a readable archive, by default into the private data folder (mode 600 on Unix; on Windows the file keeps the access list it inherits), refuses an existing file or a link at the destination and warns that the archive is plaintext. `ghostsoap forget` (M3a) lists everything it will delete (the state file, the drafts folder, drafts the state records in `--out` folders, and keyring entries), deletes them after confirmation and reports each failure separately. Docs say 'deletes', not 'erases', because an SSD cannot promise erasure.
- From M3a, `init` and each state write warn when the data folder is inside a Git working tree or a known sync folder, with the check `draft` already makes.
- Leak tests check the state file, drafts folder after `clean`, temporary files and logs for the profile's plaintext values.
- A 'What leaves your machine' page in the README lists every outbound path.

### Letters

Plain text templates in `letters/au/<lever>.txt` with `{{field}}` placeholders, rendered by a small strict renderer: an unknown placeholder is an error, and a line whose optional field is empty is dropped. Letters are written fresh from the Privacy Act, the APP guidelines and Part IIIA; none is adapted from another project's text. Each letter carries a version so a request records exactly what was sent. Subject lines carry a tag such as `[GS-7K3QX]` to match replies. From M3a, a new tag is checked against the message store, since 5 Crockford base32 characters give only 33,554,432 values.

Since M2, a draft is one message per entry and channel: an opening (date, addressee, the entry's identity fields only, a request not to use them for anything else), one paragraph per lever with its deadline and charge rule from the law table, and a closing (written reasons for a refusal, the escalation targets, the tag). Without `--lever`, a round asks for the 3 APP 7 requests, which return no personal information and need nothing from the user; any other lever is named with `--lever`. Access, correction, notification, statement, ban and unsubscribe levers need a single `--entry` and no `--pack`, and those needing the user's own words take `--detail`.

### Sending and replies

- **v0.1:** `ghostsoap draft` writes `.eml` files with `X-Unsent: 1` (so Outlook opens them as drafts) and, with `--mailto`, prints `mailto:` links for letters short enough (2,000 characters; a 3-request APP 7 letter is longer); links are opt-in because each holds the whole letter and stays in the terminal's history; `ghostsoap draft --copy` prints the To, Subject and Body for webmail, which cannot open `.eml` files and truncates long links. The user sends from their own client, then runs `ghostsoap sent <tag>` (or marks a batch). No credentials are stored, and the user's Sent folder doubles as evidence for an escalation.
- **M3a, recorded drafts:** `draft`, including `--copy`, records each message as drafted under the state lock, with its tag checked against the store, and starts no deadline. Drafting again for an entry and channel replaces its unsent draft, and `draft` names any lever the new draft drops. A draft with no recorded history keeps its tag. When a draft has withdrawn sending or request history, the new draft gets a new tag and the earlier record stays as history. A channel here is its kind and address, because some entries list 2 addresses of one kind for different requests; a draft to an address the register no longer lists for that kind is replaced too. The redraft first sets aside each verified earlier draft under a temporary name in its recorded folder. If a draft has changed, the redraft reports it and stops. If the state replacement fails, GhostSoap restores the earlier draft where it can do so without overwriting a file. The state lock stays held until the new letters are handed over. If publication fails after the state was replaced, GhostSoap reports where the earlier copy remains. Check it before removing it. Each letter stays marked unfinished in the state until every letter of its run is handed over, so `sent` refuses a letter from a run that failed or stopped until it is drafted again: the user may hold the earlier copy under the same tag. An interrupted process can leave a recovery copy for `clean` to report. After `sent`, a new draft is a new message. `sent <tag>` records the date and channel the user actually used (today and the drafted channel unless `--on` or `--channel` says otherwise), and repeating it changes nothing. A different date or channel needs `--correct`, and `sent <tag> --withdraw` takes the record back, until an answer is recorded, so a letter marked as sent by mistake is a draft again; each keeps the earlier record. `sent` refuses a date in the future, one before the letter was drafted and one after an answer already recorded. `reply <tag> <status>` records an answer for the message's requests, or for one with `--lever`, and `--source` records an upstream source the answer names.
- **M3b, replies to pushback:** `reply` also offers a ready follow-up letter for the common answers: a demand for an identity document or more identity than needed, a required channel or form, a charge for making the request or an excessive charge (APP 12.8), or any charge where the law table records that none may be made, a claim that the use is not direct marketing or that the small business exemption applies, and silence. The identity reply asks for proportionate verification, such as sighting a document or the minimum details needed (OAIC paras 12.15 and 12.17), rather than refusing every check, and never offers document numbers. Grounds are scoped by lever in a table of lever, ground, exception and citation: the reply to a required form cites paras 12.18, 12.21, 13.20 and 13.22, so it covers APP 12 and 13 only until a source is found for APP 7 and Part IIIA requests. Each follow-up carries the original tag, date and letter version.
- **M3b, reminders that escalate:** a reminder quotes the original letter's date and tag, and its wording follows the request's deadline kind. Before the date it asks for a reply by that date. After a `Days` date it states the days elapsed and that the period appears to have passed, naming the date it worked out: the period the provision sets, or the longer period the user recorded agreeing to in writing, with which it is; it adds that a public holiday where the business must act would move the last day further and GhostSoap does not know those (17 November 2026, after the design review in section 14); after a `Reasonable` guide date it says the OAIC's guide period has passed and asks for a reply; a `Reasonable` request without a guide or a `NotStated` one gets a plain follow-up that claims no period has passed. Spam Act and s 20K requests get no reply reminder. A reminder after the date names the next step: a complaint to the entity, then the escalation target.
- **M3b, complaint and escalation:** a complaint letter to the entity, because the OAIC must not investigate a complaint the complainant did not first make to the respondent, unless it considers that complaining to the respondent was not appropriate (s 40(1A)), and a request letter is not necessarily that complaint. `escalate` builds an OAIC pack only after the user records the complaint date or why complaining to the entity was not appropriate, and the pack states either; complaints about ss 20R, 20T, 21T and 21V, or a registered CR code provision relating to them, need neither (s 40(1B)). The pack is an evidence bundle the user reviews and files, never a certified complaint.
- **M3b, follow-up letters and the complaint record** (specified 17 November 2026): `follow-up <tag> [--lever]... [--complaint]` writes a reminder, or with `--complaint` the complaint letter to the business, for a message recorded as sent and not finished, in the drafts folder with the drafts manifest, `--out`, `--copy` and `clean` as `draft` has them; it records nothing in the state. Each draft is named `<entry>-<reminder|complaint>-<YYYYMMDD>-<tag>`, so `clean` and `forget` treat it as a draft. Without `--lever` a reminder covers the requests still waiting for an answer (sent or acknowledged, Spam Act and s 20K requests excepted), and a complaint those overdue or needing action after a refusal, a partial answer, a charge, an identity demand or a claimed exemption; a named request that does not qualify is refused. The complaint lists each request with what went wrong and that it is still asked, and names where the user may complain next as the request letter does: the places in the fixed list in code (decision 10) that the register entry selects, so a health insurer is not sent to AFCA, and never a place or address the register supplies; requests under ss 20R, 20T, 21T and 21V are marked as needing no complaint to the business first (s 40(1B)). It then prints the `reply <tag> escalated --to business --on <date>` line to run once the complaint is sent. `follow-up` stays separate from `reply`, which keeps the replies to pushback. The complaint is recorded on the existing `escalated` status: `reply <tag> escalated --to <target>` takes a target from a fixed list in code (`business`, `afca`, `tio`, `acma`, `oaic`) and `--on` the day the user complained; a target is required with `escalated` and refused with any other status, and a status event carries it as the optional field `to`, so format 2 is unchanged and an `escalated` recorded before the field existed still reads without one. `escalate` counts only an `escalated` to `business` still in effect, with its day, as the complaint to the business; a withdrawn one, one without a target or one to a scheme or regulator does not count.
- **M3a, a failed read is not an all-clear:** a status view that cannot read the state says DEGRADED and shows `?` for its counts, never 0. **M3b:** a removal or clear result recorded without a date is due for re-check at once, since an undated removal proves nothing.
- **M5:** SMTP through `lettre` with rustls, a review step before each batch, and a conservative rate limit.
- **M6:** read-only IMAP that matches tags and suggests a status in a review queue. It never sends, replies, deletes or follows links on its own; a confirmation link is offered only when it stays on the entity's own registered domain.

### CLI (clap 4, derive)

```text
ghostsoap init                       create the encrypted profile (M2)
ghostsoap registry list [--json]     list the packs and entries
ghostsoap plan --pack start-here     choose entities and levers; dry run by default (M2)
ghostsoap scan [--open]              list or open people-search sites' own search pages
ghostsoap draft [--format auto|eml|text] [--copy | --mailto]   write drafts or print them (M2/M3a)
ghostsoap clean                      delete drafts (M2)
ghostsoap law [--json]               print the law table and dated law status (M2)
ghostsoap check <file>               warn of legal claims the law table does not support
ghostsoap sent <tag>... [--on] [--channel] [--correct | --withdraw]   record what was sent, or take the record back (M3a)
ghostsoap reply <tag> <status> [--lever] [--source] [--note]   record an answer (M3a); offer a follow-up letter (M3b)
ghostsoap reply <tag> escalated --to <target> [--on]   record where and when you complained (M3b)
ghostsoap follow-up <tag> [--lever] [--complaint]   write a reminder or the complaint letter (M3b)
ghostsoap status [--due] [--ics <file>]   deadlines, overdue items, next actions (M3a); a calendar file (decision 11)
ghostsoap demo [--format text|html] [--html <file>]   zero-PII dry-run preview without touching the vault
ghostsoap audit [--list] [--export <file> | --manifest <file>] [--json]   verify the SHA-256 hash-chained OAIC audit log
ghostsoap escalate <tag>             export an OAIC complaint pack with formal complaint record (M3b)
ghostsoap checklist [--next] [--no-passphrase]   the hygiene checklist and progress (M4)
ghostsoap checklist done | not-for-me | clear <id>...   mark an item (M4)
ghostsoap registry check             validate the register (also run in CI)
ghostsoap export [--escalation <tag>] [--out <path>]   plaintext backup + SHA-256 manifest, or OAIC escalation bundle (M3a)
ghostsoap forget [--no-passphrase] [--yes]   delete profile, records, bundles and drafts (M3a)
ghostsoap completions <shell>        generate shell completions (bash, zsh, fish, powershell, elvish)
```

`--json` is built for `plan`, `law` and `registry list`, which hold no personal details, for the agent skill in `skills/ghostsoap`, and for `audit`, which prints the audit log's entries; another command gets it only when its milestone names it. Output honours `NO_COLOR`, and no status is shown by colour alone; M2 output has no colour.

### Dependencies

[Cargo.toml](../Cargo.toml) lists them; `jiff` replaced `chrono` on 18 November 2026, after RustSec listed chrono as unmaintained in RUSTSEC-2026-0543. Later: `keyring` and `lettre` at M5, an IMAP client chosen at M6. `serde` derive departs from the ports' `serde_json::Value` habit because the register and state have fixed schemas; the ports' reason (byte parity with Python) does not apply.

## 6. Register

- **Seed:** the 3 credit reporting bodies; the firms in the ACCC's 2024 data broker report (CoreLogic, PropTrack, Quantium, LiveRamp, Nielsen, Oracle and others named there); Australian people-search sites (leads from public broker lists, verified independently); marketing-list firms; directories; the large loyalty programmes as APP 7 targets. Target for v0.1: 25 to 40 entries at `verified` or better. Small and correct beats large and stale. On 26 October 2026 Ryan widened the register to recruiters, job boards and large consumer firms (decision 8), which takes it past that target.
- **Data licence:** CC0-1.0 for `registry/` and `checklist/`, so other open source privacy projects can reuse it; code under AGPL-3.0-or-later with section 7(b) attribution terms in `NOTICE`.
- **Freshness:** CI fails an entry with no source, an unknown lever or a malformed ABN, and warns when `verified_at` is older than 12 months. A scheduled workflow opens one issue a quarter listing stale entries. Since M1, entries carry `[[evidence]]` hashes of their source pages, and a monthly workflow opens or updates one issue listing pages that changed or could not be read, and closes it when every page matches again (normalisation in `registry/README.md`). Since 5 November 2026 the same run requests each `manual_check` page and hashes the OAIC's recognised external dispute resolution register.
- **Backlog (M3b):** unvetted leads go in `registry/au/backlog.toml` (name, lead source, date found, reason not yet an entry), so a lead is neither guessed at nor silently dropped. `registry check` prints coverage counts: entries by quality grade, kind and pack, and backlog size.
- **Contribution:** an issue form and a pull request template asking for the source URL and the date checked. Replies from `tested` requests (with personal details removed) can raise an entry's grade.
- **Feedback loop (M3a):** APP 7.6(e) replies name upstream sources; `ghostsoap reply --source "<name>"` records them, and the README invites users to report new brokers this way.
- **Overseas people-search sites (M2):** a site that lists Australians but has no Australian presence gets an entry with `exemption_risk = "overseas"` and no ABN. It carries only a `manual_check` page, the site's own web-form opt-out and an effort marker, and no levers or letters; the honesty panel says GhostSoap has not established that Australian privacy law reaches the site. The validator accepts a missing ABN only on such entries. From decision 8 the same treatment covers services whose privacy policy names an overseas company with no ABN as responsible for Australians' data (Indeed, LinkedIn), although they have Australian offices. On 13 November 2026 Ryan added business contact-data brokers (ZoomInfo, Apollo and the like), which sell work emails and phone numbers for sales and recruiting; the first 3, each run from the United States with no ABN found, are in the marketing pack. Leads may come from public broker lists (names only, as section 9 requires); existing priority tiers and difficulty ratings are the pattern for the effort marker, with no data copied.
- **Outcome reporting (M3b, opt-in):** `ghostsoap report-outcome <tag>` writes a summary with personal details removed (entry id, lever, status and days to reply in ranges rather than exact counts; a leak check refuses the summary if it carries the user's details in any common form: name slugs, phone formats and street-name abbreviations) for the user to review and paste into an issue form; nothing is sent automatically. It warns first that a public issue from the user's GitHub account shows they dealt with that entity. A maintainer reviews each report before it raises an entry to `tested` and feeds a public table of how each entity responded; Consumer Reports measured removal rates once, and no paid service publishes this.
- **Sharing with open source registers (after M1):** offer the Australian entries to compatible CC0 privacy registers, mapped to their schema. This is a pull request to another project and needs Ryan's authorisation first.

## 6a. Threat model and abuse

The threat model is kept in [SECURITY.md](../SECURITY.md#threat-model), which states what GhostSoap does today (moved there on 8 November 2026). `ghostsoap export`, so a profile survives a lost device, and `ghostsoap forget` were built on 17 November 2026. Planned and not yet built: a passphrase cache in the system keyring, off by default.

## 7. Hygiene checklist

Data under `checklist/au/`, one TOML file per item, with why it matters, the steps, the official link, when it applies, effort and a re-check interval. Initial items:

- Have I Been Pwned breach check, and IDCARE if identity documents leaked
- Credit report access from each credit reporting body, and a ban period after fraud or a breach
- Do Not Call Register
- Silent number through the user's telco
- AEC silent elector enrolment (only where there is a risk to personal safety; the user applies)
- Google 'Results about you'
- Closing dormant accounts (JustDeleteMe's MIT `sites.json` as an optional import, with its notice)
- My Health Record privacy settings
- Loyalty programme marketing opt-outs (feeds APP 7 requests)
- Social media profile audit
- AI training settings: turn off the use of what the user posts or types for training AI models, where a service offers it (added 26 November 2026, citing the OAIC's guidance for AI developers)
- ASIC director address: under ASIC's CP 391 proposal (11 September 2026), an officeholder could lodge a service address from 1 July 2027, and from 1 August 2028 an officeholder's residential address would be open to general access (free and paid extracts) unless a service address is lodged; ASIC plans its report on submissions, with a draft legislative instrument, for March 2027 (confirm the final position before inclusion)
- Doxxing or serious invasion of privacy: report to police and eSafety; a community legal centre for the statutory tort

The checklist is guidance: it links to the official page and records progress; it submits nothing.

Built on 17 November 2026: `checklist/au/` holds every item above except the ASIC director address, which waits for ASIC's final position, with credit report access and the credit ban as separate items; the JustDeleteMe import is not built. Each file gives `rank`, `title`, `why`, `steps`, `applies`, `effort` (`minutes`, `an-hour` or `longer`), an optional `recheck_months` and `[[links]]` (`name`, `url`, `checked`), the official page first; unknown fields are refused. Progress is a mark per item (`done` with its day, or `not-for-me`) in state format 2, so it is encrypted with the rest of the state; `ghostsoap checklist done`, `not-for-me` and `clear` set it. The release workflow runs `xtask checklist-checked <month>`, which fails while any link was last checked before the release month.

## 8. Milestones and acceptance checks

| Milestone | Delivers | Done when |
|---|---|---|
| M0 Scaffold | Repository, licences, README with tagline, CI | CI matrix (`ubuntu-latest`, `windows-latest`) runs `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`, `cargo build --locked --release` green; `cargo audit` and Dependabot enabled |
| M1 Register | Schema, validator, 25 to 40 entries, packs | `ghostsoap registry check` passes; CI rejects a fixture entry with no source; every entry has an ABN Lookup check recorded |
| M2 Letters and drafts | Profile, plan, draft, law table; overseas people-search entries (section 6) | Snapshot tests for every lever letter; tests show a letter contains only the entry's identity fields; deadline values verified against primary sources and cited in the law table |
| M3a Tracker core | State format 2 and its migration from format 1; messages holding one request per lever, recorded drafts, `sent` and `reply`; the transition model and 5 action states; dated deadlines and `status --due`; DEGRADED output; export and forget; the data folder check at `init` and each state write (section 5) | Unit tests for every allowed and refused status transition, and for each deadline kind at its boundaries (leap years, weekends, a recorded extension, missing holiday data); a real M2 state file migrates, and a failed migration leaves it unchanged; tests with a held lock, a wrong passphrase and a future format; a multi-lever message records mixed outcomes; repeated drafts and a repeated `sent` leave no extra message; plaintext-leak test on the state file, temporary files and the drafts folder after `clean`; DEGRADED test; export and forget round trip, with permission, existing-file and link refusal tests; CI green on Ubuntu and Windows |
| M4 Checklist | Checklist data and command | Each item links to an official page checked within the release month |
| v0.1 | First release candidate | M0 to M2, M3a and M4 met; security audit skill run, Alcuin final review, letters reviewed against the statute; release gates per the house binary-release decision when Ryan authorises a release |
| M3b Follow-ups | Replies to pushback, reminders that escalate and the complaint letter (section 5); `escalate` and the OAIC pack; re-check reminders by kind to catch re-listing, starting from the last recorded outcome or manual check, with defaults of 60 days for people-search and 90 days for other kinds (heuristics, not legal periods); opt-in `report-outcome`; planned register fields and backlog (sections 5 and 6) | Snapshot tests for every reply and reminder letter, with each ground cited from a primary source in the lever-scoped table; a reminder test for each deadline kind; `escalate` refuses without a recorded complaint date or reason except for the s 40(1B) provisions; plaintext-leak test on the outcome report, matching name, phone and street variants; undated-removal test |
| M5 SMTP | Optional sending | Review step, rate limit and keyring-only credentials, with tests against a local SMTP test server |
| M6 IMAP queue | Reply matching | Read-only by test; domain check on confirmation links |
| M7 Reach | Interface for non-technical users | Decision recorded first (section 11) |

## 9. Clean-room and provenance

- No code, letter text or data from unlicensed repositories or their forks. Public READMEs and observed behaviour informed the design only.
- CC BY-NC-SA broker directories are leads only: an entry may name a broker found there only after independent verification from the broker's own pages, and copies no text.
- MIT or Apache-2.0 sources may be reused with their notices; each reuse is listed in `NOTICE` and `docs/provenance.md`.
- GPL-3.0, AGPL-3.0 and CC BY-NC-SA projects informed design choices only (25 October and 3 November 2026); no code, text or data is copied.
- `docs/provenance.md` records, per register entry, where the lead came from and what verified it.

## 10. Repository layout

```text
GhostSoap/
  Cargo.toml  Cargo.lock  rust-toolchain.toml  clippy.toml  build.rs
  LICENSE (AGPL-3.0-or-later)  LICENSE-DATA (CC0-1.0)  NOTICE  README.md  SECURITY.md  CONTRIBUTING.md
  src/  main.rs  lib.rs  core/  io/  cli.rs
  xtask/  fuzz/  skills/ghostsoap/
  registry/au/  *.toml  packs/
  letters/au/   *.txt
  checklist/au/ *.toml (M4)
  tests/        cli.rs  registry.rs  letters.rs  workflow.rs  tracker.rs (M3a)  fixtures/  snapshots/
  docs/         plan.md  provenance.md
  .github/      workflows/ci.yml  ISSUE_TEMPLATE/  dependabot.yml
```

The GitHub repository is `yungSNAT/GhostSoap`.

## 11. Alternatives considered

1. **Fork an unlicensed Go tool.** Rejected: no licence, dormant, US-focused data, and Ryan's Rust-first rule.
2. **Contribute an Australian pack to a comparable Rust tool (Apache-2.0).** The strongest alternative: it already has an encrypted identity store, deadlines, inbox sorting and a YAML registry by region. Not chosen as the base because its registry contract closes jurisdictions, laws and templates to GDPR- or CCPA-style erasure (no AU jurisdiction), so an Australian pack would need upstream contract changes; it includes CAPTCHA-solving integrations, which break principle 4; and it is a large workspace whose direction GhostSoap cannot steer. CC0 register data keeps this open: GhostSoap's Australian entries can be offered upstream later.
3. **Contribute to a comparable EU open source web app.** Its register schema and quality grades are a sound pattern, but its requests are GDPR-shaped and it is a TypeScript web app. Possible later contribution of Australian company records.

## 12. Decisions

Ryan approved draft 2 on 24 October 2026 and decided:

1. **Licences:** Apache-2.0 for code, CC0-1.0 for register and checklist data. On 26 October 2026 the code was relicensed under FSL-1.1-ALv2 (pull request 5, merged by Ryan); the data stays CC0-1.0.
2. **Visibility:** the repository stays private until M1.
3. **Interface:** decided at M7, from a static WebAssembly web app (no install; data in the browser), a local web UI served by the CLI, or a desktop app.
4. **Branding:** the name and tagline only; no Call of Duty art, logos or fonts.
5. **Register quality (24 October 2026, at M1):** `verified` covers a check against the entity's own pages with recorded evidence, made by a person or an AI agent; provenance says which.
6. **Evidence coverage (24 October 2026, at M1):** an entry records `[[evidence]]` for each source page that gives the same hash on 2 fetches and a hash no other source shares. Pages that return bot checks, errors or browser-only content stay untracked, and provenance lists them; at M1, 22 of 29 entries carry evidence and 7 have none.
7. **Letters and drafts (25 October 2026, at M2, agent's choices within the approved plan):** the law table is Rust code with one match arm per lever, so a missing row or letter fails to compile; a message bundles an entry's levers per channel rather than one message per lever; the default round is the 3 APP 7 levers; drafts are cleaned by a `clean` command; the encrypted profile arrives in M2 (`init`), with requests and events added in M3. Ryan kept Locate Family as an overseas entry although its operator is not named.
8. **Removal requests beyond brokers (26 October 2026):** Ryan asked for data removal requests to recruitment agencies and to the companies people most often ask to delete their data. No dataset ranks companies by requests received, so he chose recruiters and job boards, firms behind large Australian data breaches and large consumer firms (telcos, banks and lenders, insurers, retailers), with a `recruitment` kind and pack. SEEK is in the register but in no pack, so a letter goes to it only with `--entry`. The agent added the kinds `telecommunications`, `financial-services`, `insurance`, `retail` and `online-service`. `app11-destroy` stays out of the default levers: the user names it with `--lever app11-destroy`, because only the user knows whether they still deal with a business (an Alcuin review found that a default would reach current customers, such as Qantas members through the loyalty pack).
9. **v0.1 without M3b (10 November 2026):** Ryan chose to ship v0.1 without M3b (section 4), as the 29 October review recommended: a reliable tracker with manual escalation guidance is worth more than an unreliable generator of reminders and OAIC packs.
10. **Tracker model (10 November 2026, at M3a):** Ryan decided that the places to complain stay a fixed list in code (the OAIC, AFCA, the TIO and the ACMA), not register data. The agent's choices within the approved plan, made after the state model review (section 14): a request's status is worked out from its events and never stored; a mistake is put right by a withdrawal record, so nothing recorded is edited or deleted; each request keeps the deadline kind it was drafted under; the state is checked when read and before it is written; a redraft matches on entry, channel kind and address; `closed` and `abandoned` are statuses rather than a field on a closed request; and the statuses `partial`, `suppressed`, `bounced` and `relisted` join the 12 the plan had.
11. **Follow-up and calendar contracts (17 November 2026, agent's choices within decisions 9 and 10):** the complaint record, `follow-up` and the calendar file are specified in section 5 so they can be built without breaking changes later; a later change to the plan can undo any of them. Their code waits on branches: the complaint record and `follow-up` until Ryan tags v0.1 or changes decision 9, and the calendar file, which is not M3b but is not in v0.1's scope either (section 4), until Ryan adds it to v0.1 or v0.1 is tagged.

## 13. Risks

| Risk | Mitigation |
|---|---|
| Wrong legal statement in a letter | Letters cite primary sources; Alcuin review before v0.1; honesty panel; no legal advice |
| Stale register | Grades, `verified_at`, CI warnings, quarterly stale issue, obsolete redirects |
| The tool becomes a dossier of the user | Encryption at rest, minimum fields, no document numbers, forget command |
| Brokers demand identity documents | Replies offer sighting or the minimum details needed (OAIC para 12.17) before any copy, and a redacted copy only where APP 12 verification reasonably needs one; record the demand in the register |
| Deliverability problems | Draft-first; small Australian list; rate limit at M5 |
| Abandonment | Small surface; data in CC0 so it survives the code |
| Trademark complaint over the name | No game assets; rename path kept simple |
| Impersonation requests | No bulk APP 12; letters invite identity verification; misuse warning (section 6a) |
| The state file syncs to the cloud | Platform data directory, never `Documents`; from M3a, `init` and each state write warn if the data folder is inside a known sync folder (until then only `draft` checks its own folder) |

## 14. Review record

- **Independent Opus review** (24 October 2026, read-only, against OAIC guidance and the Privacy Act): 'sound enough to start M0'; applied before M0. Its findings on a comparable tool were checked locally (13 CAPTCHA-solver matches in that repository's code search).
- **Statute check:** ss 6D, 20K, 20R, 20T, 21T, 21V and 40 read on AustLII on 24 October 2026.
- **Alcuin M1 final review** (24 October 2026, GPT-6 Pro at Extra High tier): 'request changes'; applied in M1, except its finding that agent checks did not meet the `verified` definition, settled by decision 5 in section 12.
- **Alcuin M2 reviews** (26 October 2026, GPT-6 at Extra High): a final review of the letters and drafts code and 7 repair reviews, ending in PASS at the head merged as pull request 3; each finding was fixed or recorded as a documented limitation before the merge.
- **Alcuin plan review** (29 October 2026, GPT-6 at Extra High; Pro was unavailable): 'proceed with named changes'; applied on 9 November 2026 in sections 4 to 13. Not adopted: its licence finding as a blocker, since the relicensing was merged and is now recorded, and rewording section 1, whose findings are already dated. Not checked: Part IIIA and Credit Reporting Code form rules, and a public holiday source for the Spam Act's business days.
- **Statute review for `ghostsoap check`** (5 November 2026, by an AI agent, not a lawyer): the Privacy Act compilation C2026C00227 (APPs 7, 11, 12 and 13; ss 20K, 20R, 20T, 21T, 21V, 35A, 40 and 41), the Spam Act compilation C2016C00614 (Sch 2 cl 6), the GDPR (Articles 3 and 17, EUR-Lex) and California Civil Code s 1798.140; its findings are the law table (section 3) and `check`'s rules. Not reviewed: Australian laws outside the law table, so `check` says only that the table's levers give no right to erasure, and whether a table deadline fits the lever a text uses it for.
- **Alcuin state model review** (10 November 2026, GPT-6 at Pro), of the format 2 types: 'change first'; applied to the M3a design (section 5). Not adopted: matching a redraft on entry and channel kind alone, since 14 of the 80 entries list 2 or more addresses of one kind for different requests; and decrypting each new state file before the rename, since the plaintext is already read back before encryption. Deferred to M3b: outcomes recorded without a date.
- **Alcuin follow-up design reviews** (16 November 2026 UTC, GPT-6 at Pro). The first, of `follow-up`, `export`, `forget` and the calendar file, approved only `export` and `forget` for v0.1. The second approved the resulting design as a specification with amendments, all applied in section 5, and found that no code for it should reach v0.1 without Ryan's decision (decision 11). Not done: import tests in Google Calendar, Outlook and Apple Calendar, which need signed-in accounts; a generated file reads back the same in 2 independent parsers (Python's `icalendar` and `vobject`).

## Sources

- [Consumer Reports evaluation of removal services](https://advocacy.consumerreports.org/press_release/consumer-reports-evaluation-of-people-search-site-removal-services-finds-that-they-are-largely-ineffective/)
- [OAIC APP 7 guidelines](https://www.oaic.gov.au/privacy/australian-privacy-principles/australian-privacy-principles-guidelines/chapter-7-app-7-direct-marketing)
- [ACCC data broker report](https://www.accc.gov.au/inquiries-and-consultations/finalised-inquiries/digital-platform-services-inquiry-2020-25/march-2024-interim-report)
- Read on 5 November 2026: [Exposure Draft, Privacy Amendment (Personal Data Protection) Bill 2026](https://consultations.ag.gov.au/rights-and-protections/privacy-reform/user_uploads/exposure-draft-bill-2026.pdf), [consultation paper](https://consultations.ag.gov.au/rights-and-protections/privacy-reform/user_uploads/consultation_paper.pdf), [consultation page](https://consultations.ag.gov.au/rights-and-protections/privacy-reform/), [Attorney-General's press conference, 31 August 2026](https://ministers.ag.gov.au/media-centre/transcripts/press-conference-blue-room-31-08-2026)
- Read on 5 November 2026: [Federal Register API](https://api.prod.legislation.gov.au/v1/versions) for the Privacy Act's versions and their reasons, [Privacy and Other Legislation Amendment Act 2024](https://www.legislation.gov.au/C2024A00128), [Interactive Gambling Amendment (Gambling Reform) Act 2026](https://www.legislation.gov.au/C2026A00072), [Telecommunications (Consumer Protection and Service Standards) Act 1999](https://www.legislation.gov.au/C2025C00623)
- Read on 6 November 2026: [OAIC, Children's Online Privacy Code](https://www.oaic.gov.au/privacy/privacy-registers/privacy-codes/childrens-online-privacy-code), [Federal Register](https://www.legislation.gov.au/), [AFCA, what is an ombudsman](https://www.afca.org.au/about-afca/corporate-information/what-is-an-ombudsman) ('We can't accept complaints about certain insurance products, including private health insurance'), [AFCA, complaints we consider](https://www.afca.org.au/what-to-expect/complaints-we-consider) (its member list), [Commonwealth Ombudsman, private health insurance complaints](https://www.ombudsman.gov.au/complaints/private-health-insurance-complaints). AFCA's Rules themselves refused the request, so this rests on AFCA's own pages
- Read on 5 November 2026: [OAIC, Handling privacy complaints](https://www.oaic.gov.au/news/blog/handling-privacy-complaints-a-new-approach-for-a-new-era), [OAIC recognised EDR schemes register](https://www.oaic.gov.au/privacy/privacy-registers/recognised-external-dispute-resolution-schemes-register), [APP 1.7-1.9 fact sheet](https://www.oaic.gov.au/__data/assets/pdf_file/0021/269013/APP-1.7-1.9-Transparency-Obligation-Fact-Sheet.PDF), [APP 1 guidelines, version 2](https://www.oaic.gov.au/__data/assets/pdf_file/0027/269064/APP-Guidelines-APP-1-Open-and-transparent-management-of-personal-information-September-2026-v2.pdf), [TIO Terms of Reference, from 1 January 2025](https://assets.tio.com.au/files/069470690e522a48de334ab0dab6b566919c61c6.pdf)
- [AEC silent electors](https://www.aec.gov.au/enrolling_to_vote/special_category/silent-electors.htm), [IDCARE credit bans](https://www.idcare.org/fact-sheets/credit-bans-australia)
- [JustDeleteMe](https://github.com/jdm-contrib/jdm), [Data Rights Protocol](https://github.com/datarightsprotocol/data-rights-protocol)
