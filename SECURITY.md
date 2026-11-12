# Security

GhostSoap handles the personal details of the person using it, so a flaw in how it stores or sends them matters.

Please report a vulnerability privately through this repository's private vulnerability reporting (Security tab, 'Report a vulnerability', or <https://github.com/yungSNAT/GhostSoap/security/advisories/new>), not in a public issue. Include the version or commit, what you did, and what you saw.

In scope: anything that exposes the user's profile, requests or replies; sends data the user did not choose to send; or weakens the encryption of the state file.

Out of scope:

- What an entity does with a request once you have sent it, and the mailbox you send it from.
- A program already running as you on the same device. It can read what you type, including the passphrase, and every file you can read.
- A wrong address or a moved page in a register entry. Report it in a public issue: entries hold no personal details.
- A flaw in a dependency that GhostSoap's code never reaches. Report it to that project.

## Threat model

What GhostSoap does today. The plan's later milestones (`docs/plan.md`) add request tracking and an export, which will be held to the same rules.

| Actor | Threat | What GhostSoap does |
|---|---|---|
| You | Lose the passphrase | Your profile is encrypted with [age](https://age-encryption.org/) (scrypt) under the passphrase alone. GhostSoap keeps no copy, cache or recovery key, so a lost passphrase cannot be recovered: delete the profile file and run `ghostsoap init` again. |
| Someone with access to your device, such as an abusive partner | Reads your profile or drafts, which show what you are trying to hide | The profile is encrypted at rest, and GhostSoap writes no log files. Drafts are plaintext `.eml` and `.txt` files, with a manifest of their names, until `ghostsoap clean` deletes them: send them, then clean. On Linux and macOS the drafts folder is created readable by you alone (0700, each file 0600); on Windows it keeps the access list it inherits. GhostSoap warns when the folder is inside a Git working tree or a known sync folder, but those checks are incomplete. Deleting a file does not wipe it from the disk. The README links to eSafety's device safety guidance. |
| An impersonator | Asks for access in your name to collect your details from many entities at once | Access requests are opt-in per entity and never part of a pack; every access letter asks the entity to verify identity before giving access (OAIC APP guidelines, paragraph 12.15); the README says asking for someone else's information may be an offence. |
| A data broker | Uses your request to confirm or enrich its record of you | A letter carries only the identity fields its register entry lists, and asks the entity to use them only to identify you and deal with the request. |
| A dependency or later change | Sends data over the network | GhostSoap makes no network connections. CI fails if a known network crate, for any platform or feature, or standard library socket use appears in the `ghostsoap` package (`cargo run -p xtask -- no-network`). The Linux tests run with only loopback; the Windows tests have the network. |
