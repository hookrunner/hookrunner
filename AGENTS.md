- The project is unreleased and under development. Support only the current code, protocol, data formats, and build layout. Backwards compatibility is explicitly unwanted.
- Keep local development files that are not necessary for the project to function, such as tests and memory banks, in the ignored `local/` directory.

- The browser is the primary delivery target. Keep gameplay and game UI in shared Rust/Bevy code so the native client has equivalent behavior when built. Limit web page code to browser bootstrapping and platform integration. Do not build the native client unless explicitly requested.

## Jira and work notes

- Track substantive project work in Jira at https://liepievar.atlassian.net, project `SCRUM` (cloud ID `43b1a13c-622d-4281-be4c-4e1646bfbd73`). Search existing issues first and update the relevant one; create a focused task when none covers the work. Include the intended outcome and acceptance criteria.
- Unless the user names another assignee, assign new tasks to Aliaksei Charnukha (account ID `712020:be5abfa9-0441-498b-b1db-364a164fc332`). Preserve existing assignments unless the user asks to change them.
- Keep issue status aligned with actual work. Use the issue's available transitions; move active work to In Progress and implemented, verified work to In Review. Mark Done only when the task's acceptance and review requirements are satisfied.
- Add concise Russian progress comments at meaningful milestones: completed work, current work, blockers, next steps, and verification results. State whether changes are local, committed, or published. Avoid duplicate tasks and repetitive comments for routine commands.
- Maintain useful Scrum notes and review agendas when relevant. Clearly label proposed meetings and pending actions; record meeting outcomes, participants, dates, and time spent only when supported by actual information. Do not invent meetings or worklogs.
- Include relevant Jira issue links when reporting results. If Jira is unavailable, continue authorized implementation and keep pending updates in the ignored `local/` directory for later synchronization.
