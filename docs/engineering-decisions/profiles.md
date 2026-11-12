# Profiles

1. What a user expects: to keep separate lives apart on one computer (work and private, or one client and another), each with its own keys, memory, skills, connectors and settings, to see which one is in use, to switch, and to remove one for good.
2. Strongest references: Hermes (MIT, 29a61cb7) `hermes_cli/profiles.py`; Ghast Workbench (closed, no licence; from our product map: each profile owns a wallet, a vault key, a database, bridge credentials, MCP configuration and sync settings).
3. Approach: a profile is a complete data folder. Everything this product keeps already lives inside its data folder (the vault key file, every database, the signing key, the approval key blob, connector tokens, settings), so two folders share nothing by construction. A service never takes a file, a project or a review target from inside the program's own data folder (which holds every profile), even when a folder above it is allowed (sv271).
4. Quality criteria: a name can never leave the profiles folder; a profile that was asked for and does not exist is an error, never a silent use of another profile's data; removal needs the name typed and is refused while a service runs there; the app says which profile it is in.

## Reading record
- Hermes `hermes_cli/profiles.py`: READ lines 275 to 512 (reserved names, the root, the sticky file, name rules, folder resolution, what counts as a live profile, listing), 1692 to 1760 (deletion: typed confirmation, stopping what holds the folder, a tombstone before removal), 1934 to 1970 (the sticky choice, written atomically). `profiles_remove.py`: outline only. NOT READ: creation with cloning (1336 to 1690), export and import, wrapper scripts, the multi-profile gateway.
- Ghast: not re-read for this component; its app code is study-only and its profile screens were not run.

## Capability matrix
| Reference does | Decision | Here |
|---|---|---|
| Hermes: the first home is `default`; named profiles under `profiles/NAME` | ADAPT | Same layout: `~/.kakurega` and `~/.kakurega/profiles/NAME` |
| Hermes: lowercase ids, reserved names | ADAPT | Lowercase letters, digits, dashes, at most 32; names kept for the program are refused |
| Hermes: a sticky active profile in a file, written atomically; `default` removes it | ADAPT | `current-profile`; a choice that is gone or malformed counts as `default` |
| Hermes: a directory is a profile only with an identity marker, so folders made by side effects are not listed | ADAPT | `profile.json` written at creation; a bare folder or a link is not a profile |
| Hermes: delete asks for the name, stops the gateway and other holders, tombstones, then removes | ADAPT, stricter | Removal needs the name (`--confirm NAME` or typed) and is refused while a service answers for the folder; nothing is killed. The marker goes first, so a removal cut short leaves no profile |
| Hermes: `-p NAME` on commands | ADAPT | `--profile NAME` is turned into the data folder once at start-up; `KAKU_PROFILE`; then the saved choice. `--data-dir` wins over all |
| Hermes: clone a profile's config, export and import | EXCLUDE for now | `kaku backup` and `kaku restore` already move one data folder with its own recovery key; copying keys between profiles is deliberately not offered |
| Hermes: wrapper commands per profile, one gateway serving many profiles | EXCLUDE | One service per profile keeps the isolation simple to state and to check |
| Ghast: a wallet per profile | EXCLUDE | No wallet in this product |

## Built
- `crates/kk-cli/src/profile.rs`: `kaku profile [list] | new NAME | use NAME | current | remove NAME [--confirm NAME]`, and the resolver every command goes through (`data_dir`).
- Service: `/api/config` gives the profile's name (read from the marker); the web header shows "Profile: NAME" for a named one.
- Desktop: starts in the profile in use; the menu-bar menu lists the profiles when there is more than one and marks the one in use; choosing another saves the choice and starts the app again in it. A named profile also keeps the shell's own state (window settings, page storage) inside its folder. Making and removing stay in the terminal.
- Evidence: kk-cli pf01 to pf05; desktop unit pr01 to pr04; `tests/e2e/profile_cli.py` 13 (two real services side by side: different signing keys, a kept conversation in one is not in the other, the order of choice, removal refused while running and without the name); `tests/e2e/desktop_profiles.mjs` 9 (the menu, the switch, the next start in the profile's folder with its own key).

## Limits
- Isolation is by folder and by key, under one operating-system user: another program running as this user can read any profile's files, as it can read the first one's.
- A service that is already running keeps the profile it was started with.
- Profiles cannot be made, renamed or removed from the web or desktop app; a profile cannot be renamed at all.
- The operating system's own records are not per profile (notification history, the Screen Recording permission).
- NOT TESTED: the real relaunch on switching (the test starts the app again itself); a packaged app; Linux and Windows.

## Owed
- Make and remove from the desktop menu and the web settings (with the same typed confirmation); rename; a per-profile label and colour so a glance tells them apart.
- `--profile` inside free text: `kaku chat explain the --profile flag` lost two words or changed profile. With the commands that take the owner's own text (ask, chat, roundtable, team, code, notes) the option is read only before the command or right after it. Evidence: `profile_option_tests`.
