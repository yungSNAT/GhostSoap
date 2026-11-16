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

What GhostSoap does today.

| Actor | Threat | What GhostSoap does |
|---|---|---|
| You | Lose the passphrase | Your profile and the record of your letters are encrypted in one state file with [age](https://age-encryption.org/) (scrypt) under the passphrase alone. GhostSoap keeps no copy, cache or recovery key, so a lost passphrase cannot be recovered: delete the state file and run `ghostsoap init` again, which loses the record of letters too. While you still know it, `ghostsoap export` writes a plaintext copy of both that you can keep somewhere safe. |
| Someone with access to your device, such as an abusive partner | Reads your profile, drafts or record of letters, which show what you are trying to hide | The profile and the record of letters (which businesses, which requests, the day each was sent and the folder each draft was written to) are encrypted at rest in the state file, and GhostSoap writes no log files. `init` and each command that records something warn when the data folder is inside a Git working tree or a known sync folder. Drafts are plaintext `.eml` and `.txt` files, with a manifest of their names, until `ghostsoap clean` deletes them: send them, then clean. `ghostsoap forget` deletes the state, default exports and the drafts it wrote once you confirm. `ghostsoap export` writes a plaintext copy of the profile and record, refuses a file or link already at its path, and warns that it is plaintext and where its folder is exposed: delete it once you no longer need it. `ghostsoap status --ics` writes a calendar file naming the businesses and requests with their dates, none of your profile, under the same rules; `forget` does not delete it. On Linux and macOS the drafts folder is created readable by you alone (0700, each file 0600); on Windows it keeps the access list it inherits. GhostSoap warns when the drafts folder is inside a Git working tree or a known sync folder too, but both checks are incomplete. Deleting a file does not wipe it from the disk. The README links to eSafety's device safety guidance. |
| An impersonator | Asks for access in your name to collect your details from many entities at once | Access requests are opt-in per entity and never part of a pack; every access letter asks the entity to verify identity before giving access (OAIC APP guidelines, paragraph 12.15); the README says asking for someone else's information may be an offence. |
| A data broker | Uses your request to confirm or enrich its record of you | A letter carries only the identity fields its register entry lists, and asks the entity to use them only to identify you and deal with the request. |
| A dependency or later change | Sends data over the network | GhostSoap makes no network connections. CI fails if a known network crate, for any platform or feature, or standard library socket use appears in the `ghostsoap` package (`cargo run -p xtask -- no-network`). The Linux tests run with only loopback; the Windows tests have the network. |
